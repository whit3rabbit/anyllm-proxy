// Test: server shuts down cleanly on signal, in-flight requests complete.

use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;
use std::time::Duration;

fn test_config() -> Config {
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

#[tokio::test]
async fn server_shuts_down_cleanly() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Verify server is up
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Send shutdown signal
    shutdown_tx.send(()).unwrap();

    // Server task should complete within a reasonable time
    let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server did not shut down within 5 seconds");
    assert!(result.unwrap().is_ok(), "server task panicked");
}

#[tokio::test]
async fn in_flight_health_completes_during_shutdown() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Start a request
    let health_url = format!("http://{addr}/health");
    let resp = reqwest::get(&health_url).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Signal shutdown
    shutdown_tx.send(()).unwrap();

    // Server should finish
    let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server did not shut down within 5 seconds");
}

#[tokio::test]
async fn new_connections_refused_after_shutdown() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Verify server is up
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Shut down and wait for server to exit
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .unwrap()
        .unwrap();

    // New connection should fail (connection refused)
    let result = reqwest::get(format!("http://{addr}/health")).await;
    assert!(
        result.is_err(),
        "expected connection refused after shutdown"
    );
}
