// Phase 9-10: compatibility endpoint and hardening integration tests

use anthropic_openai_proxy::config::{self, Config};
use anthropic_openai_proxy::server::routes;
use reqwest::Client;

fn test_config() -> Config {
    Config {
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
    }
}

async fn spawn_test_server() -> String {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn models_endpoint() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .get(format!("{base}/v1/models"))
        .header("x-api-key", "test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"].is_array());
    assert!(!body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn count_tokens_returns_unsupported() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/count_tokens"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "error");
}

#[tokio::test]
async fn batches_returns_unsupported() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/batches"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn auth_required_for_api_routes() {
    let base = spawn_test_server().await;
    let client = Client::new();
    // No auth header
    let resp = client
        .get(format!("{base}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn health_no_auth_required() {
    let base = spawn_test_server().await;
    let client = Client::new();
    // No auth header - should still work
    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn metrics_endpoint_returns_counters() {
    let base = spawn_test_server().await;
    let client = Client::new();
    // No auth required for metrics
    let resp = client.get(format!("{base}/metrics")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["requests_total"], 0);
    assert_eq!(body["requests_success"], 0);
    assert_eq!(body["requests_error"], 0);
}

// SSRF protection: validate_base_url rejects private/loopback targets
#[test]
fn ssrf_blocks_loopback() {
    assert!(config::validate_base_url("http://127.0.0.1:8080").is_err());
}

#[test]
fn ssrf_blocks_private_network() {
    assert!(config::validate_base_url("http://10.0.0.1").is_err());
    assert!(config::validate_base_url("http://172.16.0.1").is_err());
    assert!(config::validate_base_url("http://192.168.1.1").is_err());
}

#[test]
fn ssrf_blocks_localhost() {
    assert!(config::validate_base_url("http://localhost").is_err());
}

#[test]
fn ssrf_blocks_cloud_metadata() {
    assert!(config::validate_base_url("http://169.254.169.254").is_err());
    assert!(config::validate_base_url("http://metadata.google.internal").is_err());
}

#[test]
fn ssrf_allows_public_url() {
    assert!(config::validate_base_url("https://api.openai.com").is_ok());
}
