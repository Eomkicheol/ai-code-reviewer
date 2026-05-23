use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;
use crate::webhook::signature::verify_signature;

#[derive(Clone)]
pub struct AppState {
    pub webhook_secret: String,
    pub github_token: String,
}

impl AppState {
    pub fn from_env() -> Self {
        Self {
            webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
                .expect("GITHUB_WEBHOOK_SECRET not set"),
            github_token: std::env::var("GITHUB_TOKEN")
                .expect("GITHUB_TOKEN not set"),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct PrEventPayload {
    pub action: String,
    pub pull_request: PullRequest,
    pub repository: Repository,
}

#[derive(Deserialize, Debug)]
pub struct PullRequest {
    pub number: u64,
    pub head: PrHead,
}

#[derive(Deserialize, Debug)]
pub struct PrHead {
    pub sha: String,
}

#[derive(Deserialize, Debug)]
pub struct Repository {
    pub name: String,
    pub owner: Owner,
}

#[derive(Deserialize, Debug)]
pub struct Owner {
    pub login: String,
}

pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // 1. 서명 검증
    let sig = match headers.get("X-Hub-Signature-256").and_then(|v| v.to_str().ok()) {
        Some(s) => s.to_string(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if verify_signature(&body, &sig, &state.webhook_secret).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // 2. 이벤트 타입 필터
    let event = headers.get("X-GitHub-Event").and_then(|v| v.to_str().ok()).unwrap_or("");
    if event != "pull_request" {
        return StatusCode::OK.into_response();
    }

    // 3. 페이로드 파싱
    let payload: PrEventPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to parse PR payload: {e}");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    // opened / synchronize / reopened 만 처리
    if !matches!(payload.action.as_str(), "opened" | "synchronize" | "reopened") {
        return StatusCode::OK.into_response();
    }

    // 4. 파이프라인 비동기 실행 (Webhook 응답 즉시 반환)
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = run_review_pipeline(&state_clone, &payload).await {
            tracing::error!("review pipeline failed: {e}");
        }
    });

    StatusCode::ACCEPTED.into_response()
}

async fn run_review_pipeline(
    state: &AppState,
    payload: &PrEventPayload,
) -> crate::error::Result<()> {
    use crate::{
        config::loader::load_config_from_repo,
        diff::fetcher::fetch_review_contexts,
        github::GithubClient,
        llm::{claude::ClaudeProvider, openai::OpenAiProvider, gemini::GeminiProvider},
        review::{QualityReviewer, ReviewEngine, SecurityReviewer},
    };

    let owner = &payload.repository.owner.login;
    let repo = &payload.repository.name;
    let pr_number = payload.pull_request.number;
    let commit_sha = &payload.pull_request.head.sha;

    let github_client = GithubClient::new(&state.github_token, "https://api.github.com");
    let http_client = reqwest::Client::new();
    let config = load_config_from_repo(&http_client, owner, repo, &state.github_token).await?;

    let repo_info = crate::review::context::RepoInfo {
        owner: owner.clone(),
        name: repo.clone(),
        pr_number,
        commit_sha: commit_sha.clone(),
    };

    let contexts = fetch_review_contexts(&github_client, &repo_info, &config).await?;
    let model = config.provider.model.clone();

    for ctx in &contexts {
        let comments = match config.provider.name.as_str() {
            "openai" => ReviewEngine::new(
                Box::new(SecurityReviewer::new(OpenAiProvider::from_env(model.clone()))),
                Box::new(QualityReviewer::new(OpenAiProvider::from_env(model.clone()))),
            ).run(ctx).await?,
            "gemini" => ReviewEngine::new(
                Box::new(SecurityReviewer::new(GeminiProvider::from_env(model.clone()))),
                Box::new(QualityReviewer::new(GeminiProvider::from_env(model.clone()))),
            ).run(ctx).await?,
            _ => ReviewEngine::new(
                Box::new(SecurityReviewer::new(ClaudeProvider::from_env(model.clone()))),
                Box::new(QualityReviewer::new(ClaudeProvider::from_env(model.clone()))),
            ).run(ctx).await?,
        };

        if !comments.is_empty() {
            github_client
                .post_review_comments_bulk(owner, repo, pr_number, commit_sha, &comments)
                .await?;
        }
    }

    tracing::info!("review complete for {owner}/{repo}#{pr_number}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::util::ServiceExt;

    fn make_app() -> axum::Router {
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "test-secret");
        crate::webhook::router()
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
            .header("X-Hub-Signature-256", "sha256=0000000000000000000000000000000000000000000000000000000000000000")
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
