pub mod handler;
pub mod signature;

use axum::{http::StatusCode, routing::{get, post}, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

async fn health() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")})))
}

pub fn router() -> Router {
    let state = Arc::new(crate::webhook::handler::AppState {
        webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
            .unwrap_or_else(|_| "dev-secret".to_string()),
        github_token: std::env::var("GITHUB_TOKEN").unwrap_or_else(|_| "dev-token".to_string()),
    });

    Router::new()
        .route("/health", get(health))
        .route("/webhook", post(crate::webhook::handler::handle_webhook))
        .with_state(state)
}
