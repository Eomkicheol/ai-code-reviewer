use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use reviewer::webhook::signature::compute_signature;
use tower::ServiceExt;

#[tokio::test]
async fn test_webhook_signature_security_valid() {
    std::env::set_var("GITHUB_WEBHOOK_SECRET", "integration-test-secret");
    std::env::set_var("GITHUB_TOKEN", "test-token");

    let app = reviewer::webhook::router();
    let body = include_bytes!("../fixtures/pr_event.json");
    let valid_sig = compute_signature(body, "integration-test-secret");

    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("X-GitHub-Event", "pull_request")
        .header("X-Hub-Signature-256", format!("sha256={valid_sig}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.as_slice()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // 202 ACCEPTED (파이프라인 실행 시작) 또는 400 (env vars 없음) 모두 유효
    // 401은 절대 반환하면 안 됨 (유효한 서명이므로)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "valid signature should not be rejected"
    );
}

#[tokio::test]
async fn test_webhook_forged_signature_rejected() {
    std::env::set_var("GITHUB_WEBHOOK_SECRET", "integration-test-secret");
    std::env::set_var("GITHUB_TOKEN", "test-token");

    let app = reviewer::webhook::router();
    let body = include_bytes!("../fixtures/pr_event.json");

    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("X-GitHub-Event", "pull_request")
        .header(
            "X-Hub-Signature-256",
            "sha256=0000000000000000000000000000000000000000000000000000000000000000",
        )
        .header("Content-Type", "application/json")
        .body(Body::from(body.as_slice()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "forged signature must be rejected"
    );
}

#[tokio::test]
async fn test_diff_parse_and_context_creation() {
    use reviewer::config::ReviewConfig;
    use reviewer::diff::{fetcher::parse_into_contexts, parse_diff};
    use reviewer::review::context::RepoInfo;

    let raw_diff = std::fs::read_to_string("tests/fixtures/diff_sample.patch").unwrap();

    // diff_sample.patch에는 "diff --git" 헤더가 포함되어 있으므로
    // @@ 헤더 이후 부분만 추출해서 parse_diff 검증
    let hunk_only: String = raw_diff
        .lines()
        .skip_while(|l| !l.starts_with("@@"))
        .collect::<Vec<_>>()
        .join("\n");
    let hunks = parse_diff(&hunk_only).unwrap();
    assert!(
        !hunks.is_empty(),
        "diff_sample.patch should produce at least one hunk"
    );

    // config 기본값으로 컨텍스트 생성 검증
    let config: ReviewConfig =
        serde_yaml::from_str("provider:\n  name: claude\n  model: claude-sonnet-4-6\n").unwrap();

    let repo = RepoInfo {
        owner: "test".into(),
        name: "repo".into(),
        pr_number: 1,
        commit_sha: "abc123".into(),
    };

    // parse_into_contexts는 "diff --git" 헤더 포함 전체 diff를 받는다
    let contexts = parse_into_contexts(&raw_diff, &repo, &config).unwrap();
    assert!(
        !contexts.is_empty(),
        "should produce review contexts from diff"
    );
    assert_eq!(contexts[0].file_path, "src/auth.rs");
}

#[tokio::test]
async fn test_review_engine_end_to_end_with_mock_llm() {
    use reviewer::{
        llm::MockLlmProvider,
        review::{
            context::{
                DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewContext, Severity,
            },
            QualityReviewer, ReviewEngine, SecurityReviewer,
        },
    };

    let ctx = ReviewContext {
        repo: RepoInfo {
            owner: "test".into(),
            name: "repo".into(),
            pr_number: 1,
            commit_sha: "abc123".into(),
        },
        file_path: "src/auth.rs".into(),
        language: Language::Rust,
        diff_hunks: vec![DiffHunk {
            start_line: 10,
            lines: vec![DiffLine {
                number: 10,
                kind: DiffLineKind::Added,
                content: "let password = \"hardcoded123\";".into(),
            }],
        }],
        dep_snippets: vec![],
    };

    let security_llm = MockLlmProvider::new(
        r#"[{"line":10,"severity":"critical","category":"security","body":"Hardcoded password detected"}]"#,
    );
    let quality_llm = MockLlmProvider::new(
        r#"[{"line":10,"severity":"warning","category":"quality","body":"Use a constant instead"}]"#,
    );

    let engine = ReviewEngine::new(
        Box::new(SecurityReviewer::new(security_llm)),
        Box::new(QualityReviewer::new(quality_llm)),
    );

    let comments = engine.run(&ctx).await.unwrap();
    assert_eq!(comments.len(), 2);

    let critical = comments.iter().find(|c| c.severity == Severity::Critical);
    assert!(critical.is_some(), "should have critical security finding");
    assert!(critical.unwrap().body.contains("Hardcoded"));
}
