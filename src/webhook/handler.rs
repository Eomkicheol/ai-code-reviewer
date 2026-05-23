use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub webhook_secret: String,
    pub github_token: String,
}

pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    use crate::webhook::signature::verify_signature;

    let sig = match headers.get("X-Hub-Signature-256").and_then(|v| v.to_str().ok()) {
        Some(s) => s.to_string(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if verify_signature(&body, &sig, &state.webhook_secret).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let event = headers.get("X-GitHub-Event").and_then(|v| v.to_str().ok()).unwrap_or("");
    if event != "pull_request" {
        return StatusCode::OK.into_response();
    }

    StatusCode::OK.into_response()
}
