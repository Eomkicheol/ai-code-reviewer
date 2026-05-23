use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let app = reviewer::webhook::router();
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind TCP listener on :3000");
    tracing::info!(
        "listening on {}",
        listener.local_addr().expect("no local addr")
    );
    axum::serve(listener, app).await.expect("server error");
}
