// Integration tests for POST /v1/chat/completions (OpenAI-format input).

use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{routing::post, Router};
use reqwest::Client;
use serde_json::json;
use tokio::net::TcpListener;

fn openai_config_with_base(base_url: &str) -> Config {
    Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        openai_api_format: OpenAIApiFormat::Chat,
    }
}

/// Mock backend that returns a fixed OpenAI Chat Completions response.
async fn spawn_mock_chat_backend() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            axum::Json(json!({
                "id": "chatcmpl-mock123",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello from mock!"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_proxy(config: Config) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn chat_completions_non_streaming() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["id"].as_str().unwrap().starts_with("chatcmpl-"));
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");
    assert!(body["choices"][0]["message"]["content"].as_str().is_some());
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert!(body["usage"]["prompt_tokens"].as_u64().is_some());
}

#[tokio::test]
async fn chat_completions_missing_max_tokens_returns_400() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn chat_completions_empty_messages_returns_400() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn chat_completions_degradation_header_on_lossy_fields() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100,
            "presence_penalty": 0.5,
            "frequency_penalty": 0.3
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let degradation = resp
        .headers()
        .get("x-anyllm-degradation")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        degradation.contains("presence_penalty"),
        "expected presence_penalty in degradation header, got: {degradation}"
    );
    assert!(
        degradation.contains("frequency_penalty"),
        "expected frequency_penalty in degradation header, got: {degradation}"
    );
}

#[tokio::test]
async fn chat_completions_with_system_message() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
}

#[tokio::test]
async fn chat_completions_returns_openai_error_format() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    // Send completely invalid JSON
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Should have OpenAI error format (error.type, error.message)
    assert!(body["error"]["type"].is_string());
    assert!(body["error"]["message"].is_string());
}
