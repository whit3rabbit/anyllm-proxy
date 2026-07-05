// Integration tests for ANTHROPIC_FORWARD_CLIENT_AUTH (BACKEND=anthropic
// passthrough only). Drives the real /v1/messages route
// (`anthropic_passthrough`) against a mock upstream that records the
// credential header it actually received, proving:
//   - toggle off -> the operator's own configured credential always reaches
//     upstream, regardless of what the client sent (regression baseline).
//   - toggle on + PROXY_OPEN_RELAY (auth_path = OpenRelay) -> the client's
//     own header is forwarded verbatim instead, byte-for-byte (no
//     x-api-key<->Bearer re-shaping).

use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{extract::Request, response::IntoResponse, routing::post, Router};
use reqwest::Client;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

// Serializes tests in this file that mutate process env vars
// (ANTHROPIC_FORWARD_CLIENT_AUTH, PROXY_OPEN_RELAY): each test needs a
// different value, unlike thinking_repair.rs's tests which all want the same
// ones. This file compiles to its own test binary, so it can't race with
// other integration test files -- only with itself. `tokio::sync::Mutex`,
// not `std::sync::Mutex`, because the guard is held across `.await` points
// (clippy::await_holding_lock).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn anthropic_config_with_base(base_url: &str) -> Config {
    Config {
        backend: BackendKind::Anthropic,
        openai_api_key: "operator-secret-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::AnthropicApiKey("operator-secret-key".to_string()),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    }
}

async fn spawn_proxy(config: Config) -> String {
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Mock upstream Anthropic API: records the exact `x-api-key`/`authorization`
/// header it received (name + value) and acks with a minimal valid response.
async fn spawn_mock_anthropic_backend(received: Arc<Mutex<Vec<(String, String)>>>) -> String {
    let app = Router::new().route(
        "/v1/messages",
        post(move |req: Request| {
            let received = received.clone();
            async move {
                let headers = req.headers().clone();
                if let Some(v) = headers.get("x-api-key") {
                    received
                        .lock()
                        .unwrap()
                        .push(("x-api-key".to_string(), v.to_str().unwrap().to_string()));
                }
                if let Some(v) = headers.get("authorization") {
                    received
                        .lock()
                        .unwrap()
                        .push(("authorization".to_string(), v.to_str().unwrap().to_string()));
                }
                axum::Json(json!({
                    "id": "msg_ack",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "ack"}],
                    "model": "claude-opus-4-5",
                    "usage": {"input_tokens": 5, "output_tokens": 1}
                }))
                .into_response()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn toggle_off_always_forwards_operator_credential() {
    let _lock = ENV_LOCK.lock().await;
    std::env::remove_var("ANTHROPIC_FORWARD_CLIENT_AUTH");
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(received.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "client-sent-key-must-be-ignored")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        &[("x-api-key".to_string(), "operator-secret-key".to_string())],
        "toggle off: only the operator's own configured credential must reach upstream"
    );

    std::env::remove_var("PROXY_OPEN_RELAY");
}

#[tokio::test]
async fn toggle_on_with_open_relay_forwards_client_x_api_key_verbatim() {
    let _lock = ENV_LOCK.lock().await;
    std::env::set_var("ANTHROPIC_FORWARD_CLIENT_AUTH", "true");
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(received.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "clients-own-subscription-derived-key")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        &[(
            "x-api-key".to_string(),
            "clients-own-subscription-derived-key".to_string()
        )],
        "toggle on + open relay: the client's own header must be forwarded, operator credential absent"
    );

    std::env::remove_var("ANTHROPIC_FORWARD_CLIENT_AUTH");
    std::env::remove_var("PROXY_OPEN_RELAY");
}

#[tokio::test]
async fn toggle_on_with_open_relay_forwards_bearer_token_unmodified() {
    let _lock = ENV_LOCK.lock().await;
    std::env::set_var("ANTHROPIC_FORWARD_CLIENT_AUTH", "true");
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(received.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("authorization", "Bearer sk-ant-oat-subscription-token")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        &[(
            "authorization".to_string(),
            "Bearer sk-ant-oat-subscription-token".to_string()
        )],
        "must forward the Bearer token as-is -- never converted to x-api-key"
    );

    std::env::remove_var("ANTHROPIC_FORWARD_CLIENT_AUTH");
    std::env::remove_var("PROXY_OPEN_RELAY");
}

#[tokio::test]
async fn toggle_on_with_open_relay_forwards_x_goog_api_key_as_x_api_key() {
    // Regression test: validate_auth (server/middleware/auth.rs) treats
    // x-goog-api-key as equally valid to x-api-key for authentication, but
    // Anthropic's API only understands x-api-key -- the client's
    // x-goog-api-key value must reach upstream renamed to x-api-key, not
    // silently dropped in favor of the operator's own credential and not
    // forwarded under a header name Anthropic would ignore.
    let _lock = ENV_LOCK.lock().await;
    std::env::set_var("ANTHROPIC_FORWARD_CLIENT_AUTH", "true");
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(received.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-goog-api-key", "gemini-cli-compat-key")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        &[("x-api-key".to_string(), "gemini-cli-compat-key".to_string())],
        "x-goog-api-key must be forwarded upstream renamed to x-api-key, not dropped"
    );

    std::env::remove_var("ANTHROPIC_FORWARD_CLIENT_AUTH");
    std::env::remove_var("PROXY_OPEN_RELAY");
}
