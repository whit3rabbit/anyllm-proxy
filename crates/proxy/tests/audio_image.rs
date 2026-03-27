// Integration tests for audio and image passthrough endpoints.
// Actual backend calls need a live API; these tests verify routing and 501 behavior.

use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{routing::post, Router};
use reqwest::Client;
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

fn anthropic_config() -> Config {
    Config {
        backend: BackendKind::Anthropic,
        openai_api_key: String::new(),
        openai_base_url: "https://api.anthropic.com".to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: "claude-opus-4-6".into(),
            small_model: "claude-haiku-4-5".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        openai_api_format: OpenAIApiFormat::Chat,
    }
}

/// Mock backend that accepts audio and image passthrough endpoints.
async fn spawn_mock_backend() -> String {
    let app = Router::new()
        .route(
            "/v1/audio/transcriptions",
            post(|| async {
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"text":"Hello world"}"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/v1/audio/speech",
            post(|| async {
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "audio/mpeg")
                    .body(axum::body::Body::from(vec![0xFF, 0xFB, 0x90, 0x00]))
                    .unwrap()
            }),
        )
        .route(
            "/v1/images/generations",
            post(|| async {
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"created":1234567890,"data":[{"url":"https://example.com/image.png"}]}"#,
                    ))
                    .unwrap()
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_proxy_with_config(config: Config) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

// --- Audio transcriptions ---

#[tokio::test]
async fn audio_transcriptions_forwarded_to_backend() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/audio/transcriptions"))
        .header("x-api-key", "test")
        .header("content-type", "multipart/form-data; boundary=abc")
        .body("--abc\r\ncontent-disposition: form-data; name=\"file\"\r\n\r\nfake\r\n--abc--")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["text"], "Hello world");
}

#[tokio::test]
async fn audio_transcriptions_not_routed_for_anthropic_backend() {
    let proxy_base = spawn_proxy_with_config(anthropic_config()).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/audio/transcriptions"))
        .header("x-api-key", "test")
        .header("content-type", "multipart/form-data")
        .body("fake")
        .send()
        .await
        .unwrap();

    // Route not registered for Anthropic backend; fallback returns 404.
    assert_eq!(resp.status(), 404);
}

// --- Audio speech ---

#[tokio::test]
async fn audio_speech_forwarded_to_backend() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/audio/speech"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"tts-1","input":"Hello","voice":"alloy"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("audio/mpeg"), "got content-type: {ct}");
    let bytes = resp.bytes().await.unwrap();
    assert_eq!(&bytes[..], &[0xFF, 0xFB, 0x90, 0x00]);
}

// --- Image generations ---

#[tokio::test]
async fn image_generations_forwarded_to_backend() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/images/generations"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"dall-e-3","prompt":"a cat","n":1,"size":"1024x1024"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"].is_array());
    assert_eq!(body["data"][0]["url"], "https://example.com/image.png");
}

#[tokio::test]
async fn image_generations_not_routed_for_anthropic_backend() {
    let proxy_base = spawn_proxy_with_config(anthropic_config()).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/images/generations"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"dall-e-3","prompt":"a cat"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}
