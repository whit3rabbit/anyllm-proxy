use anthropic_openai_proxy::{config, server::routes};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = config::Config::from_env();
    let listen_port = config.listen_port;
    let app = routes::app(config);

    let addr = format!("0.0.0.0:{listen_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {addr}: {e}"));

    tracing::info!("proxy listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}
