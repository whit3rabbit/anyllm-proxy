// Phase 9-10: compatibility endpoint and hardening integration tests
// Phase 19: token counting integration tests

use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;
use reqwest::Client;

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
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

async fn spawn_test_server() -> String {
    // Enable open-relay mode for tests (no PROXY_API_KEYS configured).
    std::env::set_var("PROXY_OPEN_RELAY", "true");
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
async fn count_tokens_returns_count() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/count_tokens"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[{"role":"user","content":"Hello, world!"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let tokens = body["input_tokens"].as_u64().unwrap();
    assert!(tokens > 0, "expected positive token count, got {tokens}");
}

#[tokio::test]
async fn count_tokens_with_empty_messages() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/count_tokens"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["input_tokens"].is_u64());
}

#[tokio::test]
async fn count_tokens_with_tools() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/count_tokens"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[{"role":"user","content":"Use the tool"}],"tools":[{"name":"get_weather","description":"Get current weather","input_schema":{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let tokens = body["input_tokens"].as_u64().unwrap();
    // Tool definitions add tokens beyond just the message text
    assert!(
        tokens > 5,
        "expected tool schema to contribute tokens, got {tokens}"
    );
}

#[tokio::test]
async fn count_tokens_invalid_body() {
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
    // Missing required fields -> 400 invalid_request_error (Anthropic error shape)
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "invalid_request_error");
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
async fn metrics_endpoint_requires_auth() {
    let base = spawn_test_server().await;
    let client = Client::new();
    // No auth header -- should be rejected
    let resp = client.get(format!("{base}/metrics")).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn metrics_endpoint_returns_counters() {
    let base = spawn_test_server().await;
    let client = Client::new();
    // Auth required for metrics
    let resp = client
        .get(format!("{base}/metrics"))
        .header("x-api-key", "test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Multi-backend metrics format: { "backends": {...}, "total": {...} }
    assert_eq!(body["total"]["requests_total"], 0);
    assert_eq!(body["total"]["requests_success"], 0);
    assert_eq!(body["total"]["requests_error"], 0);
    assert!(body["backends"].is_object());
}

#[tokio::test]
async fn unknown_route_returns_anthropic_not_found() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .get(format!("{base}/v1/nonexistent"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "not_found_error");
    assert_eq!(body["error"]["message"], "Not found");
}

#[tokio::test]
async fn malformed_json_returns_anthropic_error() {
    let base = spawn_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("not valid json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "invalid_request_error");
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
