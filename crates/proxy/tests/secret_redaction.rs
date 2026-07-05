use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{body::Bytes, routing::post, Router};
use reqwest::Client;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const SECRET: &str = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";

fn openai_config_with_base(base_url: &str, redact_secrets: bool) -> Config {
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
        redact_secrets,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    }
}

async fn spawn_mock_chat_backend(captured_body: Arc<Mutex<Option<serde_json::Value>>>) -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let captured_body = captured_body.clone();
            async move {
                *captured_body.lock().unwrap() = Some(body);
                axum::Json(json!({
                    "id": "chatcmpl-redaction",
                    "object": "chat.completion",
                    "created": 1700000000,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "ok"},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 5, "completion_tokens": 1, "total_tokens": 6}
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_mock_images_backend(captured_body: Arc<Mutex<Option<String>>>) -> String {
    let app = Router::new().route(
        "/v1/images/generations",
        post(move |body: Bytes| {
            let captured_body = captured_body.clone();
            async move {
                *captured_body.lock().unwrap() =
                    Some(String::from_utf8(body.to_vec()).expect("request body is utf-8"));
                axum::Json(json!({
                    "created": 1700000000,
                    "data": [{"url": "https://example.test/image.png"}]
                }))
            }
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

async fn send_messages_request(base: &str) {
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 16,
            "messages": [{
                "role": "user",
                "content": format!("please keep {SECRET} private")
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn messages_redacts_secret_before_openai_upstream() {
    let captured = Arc::new(Mutex::new(None));
    let backend = spawn_mock_chat_backend(captured.clone()).await;
    let proxy = spawn_proxy(openai_config_with_base(&backend, true)).await;

    send_messages_request(&proxy).await;

    let upstream = captured.lock().unwrap().clone().expect("captured body");
    let upstream_text = serde_json::to_string(&upstream).unwrap();
    assert!(upstream_text.contains("[REDACTED_SECRET]"));
    assert!(!upstream_text.contains(SECRET));
}

#[tokio::test]
async fn messages_keeps_secret_when_redaction_disabled() {
    let captured = Arc::new(Mutex::new(None));
    let backend = spawn_mock_chat_backend(captured.clone()).await;
    let proxy = spawn_proxy(openai_config_with_base(&backend, false)).await;

    send_messages_request(&proxy).await;

    let upstream = captured.lock().unwrap().clone().expect("captured body");
    let upstream_text = serde_json::to_string(&upstream).unwrap();
    assert!(upstream_text.contains(SECRET));
}

#[tokio::test]
async fn images_redacts_secret_without_content_type_header() {
    let captured = Arc::new(Mutex::new(None));
    let backend = spawn_mock_images_backend(captured.clone()).await;
    let proxy = spawn_proxy(openai_config_with_base(&backend, true)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/images/generations"))
        .header("x-api-key", "test")
        .body(
            json!({
                "model": "gpt-image-1",
                "prompt": format!("please keep {SECRET} private")
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let upstream = captured.lock().unwrap().clone().expect("captured body");
    assert!(upstream.contains("[REDACTED_SECRET]"));
    assert!(!upstream.contains(SECRET));
}
