pub mod comment_handler;
pub mod handler;
pub mod signature;

use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

async fn health() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")})),
    )
}

pub fn router() -> Router {
    let state = Arc::new(crate::webhook::handler::AppState::from_env());

    Router::new()
        .route("/health", get(health))
        .route("/webhook", post(crate::webhook::handler::handle_webhook))
        .with_state(state)
}
