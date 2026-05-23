use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use reviewer::webhook::signature::compute_signature;
use std::sync::Mutex;
use tower::ServiceExt;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

// env var는 프로세스 전역이므로 테스트 간 동시 조작을 막는다
static ENV_LOCK: Mutex<()> = Mutex::new(());

const SAMPLE_DIFF: &str = "diff --git a/src/auth.rs b/src/auth.rs
index 1234567..abcdefg 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,3 +1,4 @@
 fn authenticate() {
+    let password = hardcoded_password_123;
 }
";

fn claude_security_response() -> serde_json::Value {
    serde_json::json!({
        "content": [{
            "type": "text",
            "text": "[{\"line\":2,\"severity\":\"critical\",\"category\":\"security\",\"body\":\"Hardcoded credential detected\"}]"
        }]
    })
}

#[tokio::test]
async fn test_full_pr_pipeline_end_to_end() {
    let _lock = ENV_LOCK.lock().unwrap();

    let github_mock = MockServer::start().await;
    let claude_mock = MockServer::start().await;

    // .reviewbot.yml 없음 → 기본값(claude provider) 사용
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/contents/.reviewbot.yml"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&github_mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .and(header("Accept", "application/vnd.github.v3.diff"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SAMPLE_DIFF))
        .mount(&github_mock)
        .await;

    // dep snippets → 404 (non-fatal, 파이프라인 계속)
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/contents/src/auth.rs"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&github_mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/pulls/42/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 1})))
        .mount(&github_mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/pulls/42/reviews"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 1})))
        .mount(&github_mock)
        .await;

    // security + quality + summary 세 호출 모두 동일 응답
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sim-claude-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(claude_security_response()),
        )
        .mount(&claude_mock)
        .await;

    std::env::set_var("GITHUB_WEBHOOK_SECRET", "sim-secret");
    std::env::set_var("GITHUB_TOKEN", "sim-token");
    std::env::set_var("GITHUB_API_URL", &github_mock.uri());
    std::env::set_var("CLAUDE_API_KEY", "sim-claude-key");
    std::env::set_var("CLAUDE_BASE_URL", &claude_mock.uri());
    std::env::set_var("DB_PATH", ":memory:");

    let app = reviewer::webhook::router();

    let pr_payload = serde_json::json!({
        "action": "opened",
        "pull_request": {
            "number": 42,
            "head": {"sha": "abc123def456abc123"}
        },
        "repository": {
            "name": "test-repo",
            "owner": {"login": "test-owner"}
        }
    });

    let body_bytes = serde_json::to_vec(&pr_payload).unwrap();
    let sig = compute_signature(&body_bytes, "sim-secret");

    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("X-GitHub-Event", "pull_request")
        .header("X-Hub-Signature-256", format!("sha256={sig}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "PR 웹훅은 파이프라인 시작과 함께 202를 즉시 반환해야 한다"
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let claude_reqs = claude_mock.received_requests().await.unwrap();
    assert!(
        claude_reqs.len() >= 2,
        "Claude API 최소 2회(security + quality) 호출 필요, 실제: {}",
        claude_reqs.len()
    );

    let github_reqs = github_mock.received_requests().await.unwrap();
    let review_posted = github_reqs
        .iter()
        .any(|r| r.url.path().ends_with("/reviews"));
    assert!(
        review_posted,
        "PR 리뷰 미게시. 수신 요청: {:#?}",
        github_reqs
            .iter()
            .map(|r| format!("{} {}", r.method, r.url.path()))
            .collect::<Vec<_>>()
    );

    let comment_posted = github_reqs
        .iter()
        .any(|r| r.url.path().ends_with("/comments"));
    assert!(comment_posted, "PR 줄 코멘트 미게시");
}

#[tokio::test]
async fn test_bot_comment_is_ignored() {
    let _lock = ENV_LOCK.lock().unwrap();

    std::env::set_var("GITHUB_WEBHOOK_SECRET", "sim-secret");
    std::env::set_var("GITHUB_TOKEN", "sim-token");
    // 봇 댓글은 핸들러에서 즉시 반환하므로 API 호출 없음
    std::env::set_var("GITHUB_API_URL", "https://api.github.com");

    let app = reviewer::webhook::router();

    let bot_payload = serde_json::json!({
        "action": "created",
        "comment": {
            "id": 999,
            "body": "/review",
            "user": {"login": "github-actions[bot]", "type": "Bot"}
        },
        "issue": {
            "number": 7,
            "pull_request": {"url": "https://api.github.com/repos/owner/repo/pulls/7"}
        },
        "repository": {
            "name": "test-repo",
            "owner": {"login": "test-owner"}
        }
    });

    let body_bytes = serde_json::to_vec(&bot_payload).unwrap();
    let sig = compute_signature(&body_bytes, "sim-secret");

    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("X-GitHub-Event", "issue_comment")
        .header("X-Hub-Signature-256", format!("sha256={sig}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "봇 댓글은 파이프라인 없이 200으로 즉시 무시되어야 한다"
    );
}
