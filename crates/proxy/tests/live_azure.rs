//! Live integration tests against Azure OpenAI endpoints.
//!
//! All tests are `#[ignore]` so they never run in CI or default `cargo test`.
//!
//! Run manually:
//! ```sh
//! AZURE_OPENAI_API_KEY=... \
//! AZURE_OPENAI_ENDPOINT=https://your-resource.openai.azure.com \
//! AZURE_OPENAI_DEPLOYMENT=your-deployment \
//!   cargo test --test live_azure -- --ignored --test-threads=1
//! ```

use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;
use serde_json::{json, Value};
use tokio::net::TcpListener;

fn azure_test_config() -> Config {
    let api_key = std::env::var("AZURE_OPENAI_API_KEY")
        .expect("AZURE_OPENAI_API_KEY must be set for live Azure tests");
    let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT")
        .expect("AZURE_OPENAI_ENDPOINT must be set for live Azure tests");
    let deployment = std::env::var("AZURE_OPENAI_DEPLOYMENT")
        .expect("AZURE_OPENAI_DEPLOYMENT must be set for live Azure tests");
    let api_version =
        std::env::var("AZURE_OPENAI_API_VERSION").unwrap_or_else(|_| "2024-10-21".to_string());

    let base_url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        endpoint.trim_end_matches('/'),
        deployment,
        api_version
    );

    Config {
        backend: config::BackendKind::AzureOpenAI,
        openai_api_key: String::new(),
        openai_base_url: base_url,
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: deployment.clone(),
            small_model: deployment,
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::AzureApiKey(api_key),
        log_bodies: true,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

async fn spawn_test_server(config: Config) -> String {
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Verify a basic non-streaming request through Azure OpenAI.
#[tokio::test]
#[ignore]
async fn azure_non_streaming_hello() {
    let base = spawn_test_server(azure_test_config()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "Say hello in exactly one word."}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "body: {}", resp.text().await.unwrap());
}

/// Verify streaming through Azure OpenAI produces SSE events.
#[tokio::test]
#[ignore]
async fn azure_streaming_hello() {
    let base = spawn_test_server(azure_test_config()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "Say hello in exactly one word."}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("event: message_start"),
        "expected SSE events in: {body}"
    );
    assert!(
        body.contains("event: message_stop"),
        "expected message_stop in: {body}"
    );
}
