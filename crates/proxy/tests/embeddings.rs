// Integration tests for POST /v1/embeddings passthrough and x-anyllm-degradation header.

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
        expose_degradation_warnings: false,
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
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
    }
}

/// Start a mock backend that accepts POST /v1/embeddings and returns a fixed response.
/// Returns the base URL of the mock server.
async fn spawn_mock_backend() -> String {
    let app = Router::new().route(
        "/v1/embeddings",
        post(|| async {
            axum::response::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1,0.2,0.3]}],"model":"text-embedding-3-small","usage":{"prompt_tokens":5,"total_tokens":5}}"#,
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

#[tokio::test]
async fn embeddings_forwarded_to_backend() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/embeddings"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"text-embedding-3-small","input":"hello world"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "list");
    assert!(body["data"].is_array());
}

#[tokio::test]
async fn embeddings_response_content_type_forwarded() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/embeddings"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"text-embedding-3-small","input":"hello"}"#)
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
    assert!(ct.contains("application/json"), "got content-type: {ct}");
}

#[tokio::test]
async fn embeddings_requires_auth() {
    let mock_base = spawn_mock_backend().await;
    let proxy_base = spawn_proxy_with_config(openai_config_with_base(&mock_base)).await;

    // Temporarily unset open-relay mode so auth is enforced
    std::env::remove_var("PROXY_OPEN_RELAY");
    let proxy_strict = {
        let mut c = openai_config_with_base(&mock_base);
        c.openai_api_key = "sk-real".into();
        spawn_proxy_with_config(c).await
    };

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_strict}/v1/embeddings"))
        .header("content-type", "application/json")
        .body(r#"{"model":"text-embedding-3-small","input":"hello"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);

    // Restore for other tests
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    // Silence the unused-variable warning — proxy_base was used above
    let _ = proxy_base;
}

#[tokio::test]
async fn embeddings_not_routed_for_anthropic_backend() {
    // Anthropic backend does not mount /v1/embeddings; route returns 404.
    let proxy_base = spawn_proxy_with_config(anthropic_config()).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_base}/v1/embeddings"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(r#"{"model":"text-embedding-3-small","input":"hello"}"#)
        .send()
        .await
        .unwrap();

    // Route not registered for Anthropic backend; fallback returns 404.
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn degradation_header_present_when_top_k_set() {
    // We can't make a full messages round-trip without a real backend, but we can
    // verify the compute_request_warnings function directly via the translator crate.
    // The proxy-level injection is covered by the inject_degradation_header unit test.
    use anyllm_translate::anthropic;
    let req = anthropic::MessageCreateRequest {
        model: "claude-sonnet-4-6".to_string(),
        max_tokens: 100,
        messages: vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Text("hi".to_string()),
        }],
        system: None,
        temperature: None,
        top_p: None,
        top_k: Some(40),
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: None,
        extra: serde_json::Map::new(),
    };

    let warnings = anyllm_translate::compute_request_warnings(&req);
    let header_val = warnings.as_header_value().expect("should have warnings");
    assert!(header_val.contains("top_k"), "got: {header_val}");
}

#[tokio::test]
async fn degradation_header_absent_when_no_lossy_features() {
    use anyllm_translate::anthropic;
    let req = anthropic::MessageCreateRequest {
        model: "claude-sonnet-4-6".to_string(),
        max_tokens: 100,
        messages: vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Text("hi".to_string()),
        }],
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: None,
        extra: serde_json::Map::new(),
    };

    let warnings = anyllm_translate::compute_request_warnings(&req);
    assert!(warnings.is_empty());
    assert!(warnings.as_header_value().is_none());
}
