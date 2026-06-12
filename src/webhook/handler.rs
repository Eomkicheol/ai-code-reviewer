use crate::webhook::signature::verify_signature;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub webhook_secret: String,
    pub github_token: String,
    pub github_api_url: String,
}

impl AppState {
    pub fn from_env() -> Self {
        Self {
            webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
                .expect("GITHUB_WEBHOOK_SECRET not set"),
            github_token: std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set"),
            github_api_url: std::env::var("GITHUB_API_URL")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PrEventPayload {
    pub action: String,
    pub pull_request: PullRequest,
    pub repository: Repository,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub head: PrHead,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PrHead {
    pub sha: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Repository {
    pub name: String,
    pub owner: Owner,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Owner {
    pub login: String,
}

pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // 서명 검증
    let sig = match headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s.to_string(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if verify_signature(&body, &sig, &state.webhook_secret).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let event = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match event {
        "pull_request" => handle_pull_request_event(state, body),
        "issue_comment" => handle_issue_comment_event(state, body),
        _ => StatusCode::OK.into_response(),
    }
}

/// pull_request 이벤트 처리 — opened/synchronize/reopened 시 리뷰 파이프라인 실행
fn handle_pull_request_event(state: Arc<AppState>, body: Bytes) -> axum::response::Response {
    let payload: PrEventPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to parse PR payload: {e}");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    // opened / synchronize / reopened 만 처리
    if !matches!(
        payload.action.as_str(),
        "opened" | "synchronize" | "reopened"
    ) {
        return StatusCode::OK.into_response();
    }

    // 파이프라인 비동기 실행 (Webhook 응답 즉시 반환)
    tokio::spawn(async move {
        if let Err(e) = run_review_pipeline(&state, &payload, None).await {
            tracing::error!("review pipeline failed: {e}");
        }
    });

    StatusCode::ACCEPTED.into_response()
}

/// issue_comment 이벤트 처리 — PR 댓글 명령(/review, @reviewer) 처리
fn handle_issue_comment_event(state: Arc<AppState>, body: Bytes) -> axum::response::Response {
    use crate::webhook::comment_handler::{parse_command, CommentCommand, IssueCommentPayload};

    let payload: IssueCommentPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to parse issue_comment payload: {e}");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    // PR 댓글만 처리 (일반 issue 댓글 무시)
    if payload.issue.pull_request.is_none() {
        return StatusCode::OK.into_response();
    }

    // 봇 자신의 댓글 무시 (무한루프 방지)
    if payload.comment.user.user_type == "Bot" {
        return StatusCode::OK.into_response();
    }

    // created 이벤트만 처리
    if payload.action != "created" {
        return StatusCode::OK.into_response();
    }

    let cmd = parse_command(&payload.comment.body, "reviewer");
    if cmd == CommentCommand::Unknown {
        return StatusCode::OK.into_response();
    }

    // TargetedReview 타겟 문자열을 소유권 이전 전에 추출
    let review_target: Option<String> = if let CommentCommand::TargetedReview(ref t) = cmd {
        Some(t.clone())
    } else {
        None
    };

    tokio::spawn(async move {
        let owner = &payload.repository.owner.login;
        let repo = &payload.repository.name;
        let pr_number = payload.issue.number;
        let github_client =
            match crate::github::GithubClient::new(&state.github_token, &state.github_api_url) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("GithubClient 생성 실패: {e}");
                    return;
                }
            };

        match cmd {
            CommentCommand::FullReview | CommentCommand::TargetedReview(_) => {
                dispatch_review_command(
                    &state,
                    &github_client,
                    owner,
                    repo,
                    pr_number,
                    review_target.as_deref(),
                    payload.repository.clone(),
                )
                .await
            }
            CommentCommand::Question(question) => {
                dispatch_question_command(&github_client, owner, repo, pr_number, &question).await
            }
            CommentCommand::Unknown => {}
        }
    });

    StatusCode::ACCEPTED.into_response()
}

/// /review 명령 처리 — PR head SHA 조회 후 리뷰 파이프라인 실행
async fn dispatch_review_command(
    state: &AppState,
    github_client: &crate::github::GithubClient,
    owner: &str,
    repo: &str,
    pr_number: u64,
    review_target: Option<&str>,
    repository: Repository,
) {
    if let Some(t) = review_target {
        tracing::info!("/review {t} 명령 수신: {owner}/{repo}#{pr_number}");
    } else {
        tracing::info!("/review 명령 수신: {owner}/{repo}#{pr_number}");
    }

    if let Err(e) = github_client
        .post_issue_comment(
            owner,
            repo,
            pr_number,
            "리뷰를 시작합니다. 잠시만 기다려주세요...",
        )
        .await
    {
        tracing::error!("리뷰 시작 댓글 게시 실패: {e}");
    }

    // SHA 조회 실패 시 빈 문자열로 진행하지 않고 중단
    let head_sha = match github_client.get_pr_head_sha(owner, repo, pr_number).await {
        Ok(sha) => sha,
        Err(e) => {
            tracing::error!("PR head SHA 조회 실패, 리뷰 중단: {e}");
            return;
        }
    };

    let pr_payload = PrEventPayload {
        action: "opened".to_string(),
        pull_request: PullRequest {
            number: pr_number,
            head: PrHead { sha: head_sha },
        },
        repository,
    };

    if let Err(e) = run_review_pipeline(state, &pr_payload, review_target).await {
        tracing::error!("/review 파이프라인 실패: {e}");
    }
}

/// @reviewer 질문 명령 처리 — LLM으로 질문 답변 후 댓글 게시
/// github_client를 재사용하므로 별도 AppState 파라미터 불필요
async fn dispatch_question_command(
    github_client: &crate::github::GithubClient,
    owner: &str,
    repo: &str,
    pr_number: u64,
    question: &str,
) {
    tracing::info!("@reviewer 질문 수신 {owner}/{repo}#{pr_number}: {question}");

    use crate::config::loader::load_config_from_repo;
    // 이미 생성된 github_client 재사용 — 별도 reqwest::Client 생성 불필요
    let config = match load_config_from_repo(github_client, owner, repo).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("질문 핸들러 설정 로드 실패: {e}");
            return;
        }
    };

    // provider factory로 LLM 생성
    let llm = crate::llm::create_provider(&config.provider.name, config.provider.model.clone());
    let answer_result = crate::webhook::comment_handler::handle_question(
        &llm,
        question,
        owner,
        repo,
        pr_number,
        github_client,
    )
    .await;

    match answer_result {
        Ok(answer) => {
            let body = format!("> {question}\n\n{answer}\n\n*AI Code Reviewer*");
            if let Err(e) = github_client
                .post_issue_comment(owner, repo, pr_number, &body)
                .await
            {
                tracing::error!("질문 답변 게시 실패: {e}");
            }
        }
        Err(e) => tracing::error!("질문 답변 생성 실패: {e}"),
    }
}

async fn run_review_pipeline(
    state: &AppState,
    payload: &PrEventPayload,
    target: Option<&str>,
) -> crate::error::Result<()> {
    use crate::{
        config::loader::load_config_from_repo,
        context::ContextStore,
        diff::fetcher::fetch_review_contexts,
        github::GithubClient,
        review::{summary::generate_pr_summary, QualityReviewer, ReviewEngine, SecurityReviewer},
    };

    let owner = &payload.repository.owner.login;
    let repo = &payload.repository.name;
    let pr_number = payload.pull_request.number;
    let commit_sha = &payload.pull_request.head.sha;
    let repo_key = format!("{owner}/{repo}");

    let github_client = GithubClient::new(&state.github_token, &state.github_api_url)?;
    // github_client 재사용 — 별도 reqwest::Client 생성 및 SSRF 우회 없음
    let config = load_config_from_repo(&github_client, owner, repo).await?;

    // 컨텍스트 스토어 초기화 — DB 실패는 non-fatal (리뷰 자체는 계속 진행)
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "reviewer.db".to_string());
    let store = match ContextStore::new(&db_path).await {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("컨텍스트 DB 초기화 실패 (무시하고 계속): {e}");
            None
        }
    };

    let past_patterns = if let Some(ref s) = store {
        s.get_patterns(&repo_key).await.unwrap_or_default()
    } else {
        vec![]
    };

    let pattern_hint = if past_patterns.is_empty() {
        String::new()
    } else {
        tracing::info!("과거 패턴 {}개 로드: {owner}/{repo}", past_patterns.len());
        // 섹션 헤더는 generate_pr_summary 내부에서 추가하므로 여기서는 내용만 전달
        past_patterns
            .iter()
            .map(|p| {
                format!(
                    "- [{}] {} ({}회 발견)",
                    p.category, p.description, p.occurrence_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let repo_info = crate::review::context::RepoInfo {
        owner: owner.clone(),
        name: repo.clone(),
        pr_number,
        commit_sha: commit_sha.clone(),
    };

    let contexts = fetch_review_contexts(&github_client, &repo_info, &config).await?;
    let model = config.provider.model.clone();

    let mut all_comments = Vec::new();

    let (run_security, run_quality) = match target {
        None => (true, true),
        Some("security") => (true, false),
        Some("quality") => (false, true),
        Some(t) => {
            tracing::warn!("알 수 없는 리뷰 타겟 '{t}', 전체 리뷰 실행");
            (true, true)
        }
    };

    for ctx in &contexts {
        let security_enabled = config.reviewers.security.enabled && run_security;
        let quality_enabled = config.reviewers.quality.enabled && run_quality;

        // provider factory로 3중 match 제거
        // 비활성 리뷰어는 MockLlmProvider("[]")로 대체하여 빈 결과 반환
        let security_llm: Box<dyn crate::llm::LlmProvider + Send + Sync> = if security_enabled {
            crate::llm::create_provider(&config.provider.name, model.clone())
        } else {
            Box::new(crate::llm::MockLlmProvider::new("[]"))
        };
        let quality_llm: Box<dyn crate::llm::LlmProvider + Send + Sync> = if quality_enabled {
            crate::llm::create_provider(&config.provider.name, model.clone())
        } else {
            Box::new(crate::llm::MockLlmProvider::new("[]"))
        };
        let comments = ReviewEngine::new(
            Box::new(SecurityReviewer::new(security_llm)),
            Box::new(QualityReviewer::new(quality_llm)),
        )
        .run(ctx)
        .await?;

        // severity_threshold 필터 적용
        let sec_threshold = &config.reviewers.security.severity_threshold;
        let qual_threshold = &config.reviewers.quality.severity_threshold;
        let comments: Vec<_> = comments
            .into_iter()
            .filter(|c| {
                let threshold = match c.category {
                    crate::review::Category::Security => sec_threshold.as_str(),
                    _ => qual_threshold.as_str(),
                };
                severity_meets_threshold(&c.severity, threshold)
            })
            .collect();

        // 발견된 패턴 DB에 저장 (실패해도 파이프라인 중단하지 않음)
        if let Some(ref s) = store {
            for comment in &comments {
                s.record_findings(
                    &repo_key,
                    &ctx.file_path,
                    &format!("{:?}", comment.category),
                    &comment.body,
                )
                .await
                .unwrap_or_else(|e| tracing::warn!("패턴 저장 실패: {e}"));
            }
        }

        if !comments.is_empty() {
            // 개별 댓글 실패 시 나머지 계속 게시 (부분 성공 허용)
            github_client
                .post_review_comments_bulk(owner, repo, pr_number, commit_sha, &comments)
                .await
                .unwrap_or_else(|e| tracing::warn!("코멘트 게시 부분 실패: {e}"));
        }

        all_comments.extend(comments);
    }

    // provider factory로 3중 match 제거
    let summary_llm = crate::llm::create_provider(&config.provider.name, model.clone());
    let summary = generate_pr_summary(
        &summary_llm,
        &all_comments,
        &repo_key,
        pr_number,
        &pattern_hint,
    )
    .await?;
    github_client
        .create_pr_review(owner, repo, pr_number, commit_sha, &summary)
        .await?;

    tracing::info!(
        "review complete for {owner}/{repo}#{pr_number} — {}개 발견",
        all_comments.len()
    );
    Ok(())
}

fn severity_meets_threshold(severity: &crate::review::Severity, threshold: &str) -> bool {
    use crate::review::Severity;
    match threshold {
        "critical" => matches!(severity, Severity::Critical),
        "warning" => matches!(severity, Severity::Critical | Severity::Warning),
        _ => true, // "info" 또는 기타 → 모두 포함
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Severity;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    fn make_app() -> axum::Router {
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "test-secret");
        std::env::set_var("GITHUB_TOKEN", "test-token");
        crate::webhook::router()
    }

    #[test]
    fn test_severity_threshold_critical_only() {
        assert!(severity_meets_threshold(&Severity::Critical, "critical"));
        assert!(!severity_meets_threshold(&Severity::Warning, "critical"));
        assert!(!severity_meets_threshold(&Severity::Info, "critical"));
    }

    #[test]
    fn test_severity_threshold_warning_and_above() {
        assert!(severity_meets_threshold(&Severity::Critical, "warning"));
        assert!(severity_meets_threshold(&Severity::Warning, "warning"));
        assert!(!severity_meets_threshold(&Severity::Info, "warning"));
    }

    #[test]
    fn test_severity_threshold_info_allows_all() {
        assert!(severity_meets_threshold(&Severity::Critical, "info"));
        assert!(severity_meets_threshold(&Severity::Warning, "info"));
        assert!(severity_meets_threshold(&Severity::Info, "info"));
    }

    #[test]
    fn test_severity_threshold_unknown_allows_all() {
        // 알 수 없는 임계값 → 모두 허용 (안전 폴백)
        assert!(severity_meets_threshold(&Severity::Critical, "unknown"));
        assert!(severity_meets_threshold(&Severity::Info, "unknown"));
    }

    #[test]
    fn test_appstate_defaults_github_api_url() {
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "s");
        std::env::set_var("GITHUB_TOKEN", "t");
        std::env::remove_var("GITHUB_API_URL");
        let state = AppState::from_env();
        assert_eq!(state.github_api_url, "https://api.github.com");
    }

    #[test]
    fn test_appstate_custom_github_api_url() {
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "s");
        std::env::set_var("GITHUB_TOKEN", "t");
        std::env::set_var("GITHUB_API_URL", "https://github.example.com/api/v3");
        let state = AppState::from_env();
        assert_eq!(state.github_api_url, "https://github.example.com/api/v3");
        std::env::remove_var("GITHUB_API_URL");
    }

    #[tokio::test]
    async fn test_webhook_missing_signature_returns_401() {
        let app = make_app();
        let req = Request::builder()
            .method("POST")
            .uri("/webhook")
            .header("X-GitHub-Event", "pull_request")
            .header("Content-Type", "application/json")
            .body(Body::from("{}"))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_webhook_invalid_signature_returns_401() {
        let app = make_app();
        let req = Request::builder()
            .method("POST")
            .uri("/webhook")
            .header("X-GitHub-Event", "pull_request")
            .header(
                "X-Hub-Signature-256",
                "sha256=0000000000000000000000000000000000000000000000000000000000000000",
            )
            .header("Content-Type", "application/json")
            .body(Body::from("{}"))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_non_pr_event_returns_200_no_op() {
        use crate::webhook::signature::compute_signature;
        let app = make_app();
        let body = b"{}";
        let sig = compute_signature(body, "test-secret");
        let req = Request::builder()
            .method("POST")
            .uri("/webhook")
            .header("X-GitHub-Event", "push")
            .header("X-Hub-Signature-256", format!("sha256={sig}"))
            .header("Content-Type", "application/json")
            .body(Body::from(body.as_slice()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
