// Integration tests for multi-backend path-prefix routing.

use anthropic_openai_proxy::config::MultiConfig;
use anthropic_openai_proxy::server::routes;
use reqwest::Client;

fn test_multi_config() -> MultiConfig {
    // Two backends: openai (default) and gemini
    let toml = r#"
        default_backend = "openai"

        [backends.openai]
        kind = "openai"
        api_key = "sk-test-openai"

        [backends.gemini]
        kind = "gemini"
        api_key = "test-gemini-key"
    "#;
    MultiConfig::from_toml_str(toml)
}

async fn spawn_multi_server() -> String {
    // Enable open-relay mode for tests (no PROXY_API_KEYS configured).
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app_multi(test_multi_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn health_works_with_multi_backend() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn default_route_requires_auth() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    // /v1/messages without auth should return 401
    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":100,"messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn prefixed_route_requires_auth() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    // /openai/v1/messages without auth should return 401
    let resp = client
        .post(format!("{base}/openai/v1/messages"))
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":100,"messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn prefixed_models_endpoint_works() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    // /openai/v1/models should work with auth
    let resp = client
        .get(format!("{base}/openai/v1/models"))
        .header("x-api-key", "any-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"].is_array());
}

#[tokio::test]
async fn gemini_prefixed_models_endpoint_works() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client
        .get(format!("{base}/gemini/v1/models"))
        .header("x-api-key", "any-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn unknown_prefix_returns_404() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client
        .get(format!("{base}/unknown/v1/models"))
        .header("x-api-key", "any-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn metrics_shows_per_backend_breakdown() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client
        .get(format!("{base}/metrics"))
        .header("x-api-key", "any-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Should have per-backend metrics
    assert!(body["backends"]["openai"].is_object());
    assert!(body["backends"]["gemini"].is_object());
    // And totals
    assert_eq!(body["total"]["requests_total"], 0);
}

#[tokio::test]
async fn default_route_count_tokens_works() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages/count_tokens"))
        .header("x-api-key", "any-key")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":100,"messages":[{"role":"user","content":"hello world"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["input_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn prefixed_count_tokens_works() {
    let base = spawn_multi_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/openai/v1/messages/count_tokens"))
        .header("x-api-key", "any-key")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-sonnet-4-6","max_tokens":100,"messages":[{"role":"user","content":"hello world"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
