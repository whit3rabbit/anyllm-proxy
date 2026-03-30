//! Live API integration tests against real OpenAI endpoints.
//!
//! All tests are `#[ignore]` so they never run in CI or default `cargo test`.
//!
//! Run manually:
//! ```sh
//! OPENAI_API_KEY=sk-... cargo test --test live_api -- --ignored --test-threads=1
//! ```

use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Build a Config targeting the real OpenAI Chat Completions API.
/// Reads OPENAI_API_KEY from the environment; panics if absent.
fn test_config() -> Config {
    let api_key =
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for live tests");
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: api_key.clone(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o-mini".to_string(),
            small_model: "gpt-4o-mini".to_string(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken(api_key),
        log_bodies: false,
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

/// Spawn the proxy on a random port, return the base URL (e.g. "http://127.0.0.1:12345").
async fn spawn_test_server(config: Config) -> String {
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Helper: build a reqwest client with the x-api-key header.
fn api_client() -> reqwest::Client {
    reqwest::Client::new()
}

// ---------------------------------------------------------------------------
// a) Non-streaming text completion
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_openai_text() {
    let base = spawn_test_server(test_config()).await;
    let client = api_client();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 50,
            "messages": [{"role": "user", "content": "Say hello in exactly one word."}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "expected 200, got {}", resp.status());

    let body: Value = resp.json().await.expect("response is not valid JSON");

    assert_eq!(body["type"], "message", "type must be 'message'");
    assert_eq!(body["role"], "assistant", "role must be 'assistant'");
    assert_eq!(
        body["stop_reason"], "end_turn",
        "stop_reason must be 'end_turn'"
    );

    // Content array must be non-empty with text.
    let content = body["content"].as_array().expect("content must be array");
    assert!(!content.is_empty(), "content array must not be empty");
    let first = &content[0];
    assert_eq!(first["type"], "text");
    let text = first["text"].as_str().expect("text must be a string");
    assert!(!text.is_empty(), "text must not be empty");

    // Usage must have positive token counts.
    let usage = &body["usage"];
    assert!(
        usage["input_tokens"].as_u64().unwrap_or(0) > 0,
        "input_tokens must be > 0"
    );
    assert!(
        usage["output_tokens"].as_u64().unwrap_or(0) > 0,
        "output_tokens must be > 0"
    );
}

// ---------------------------------------------------------------------------
// b) Streaming SSE
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_openai_streaming() {
    let base = spawn_test_server(test_config()).await;
    let client = api_client();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 60,
            "stream": true,
            "messages": [{"role": "user", "content": "Say hello in exactly one word."}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);

    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "expected text/event-stream, got: {ct}"
    );

    // Read the full SSE body and parse event types.
    let body = resp.text().await.expect("failed to read SSE body");
    let event_types: Vec<&str> = body
        .lines()
        .filter_map(|line| line.strip_prefix("event: "))
        .collect();

    assert!(
        !event_types.is_empty(),
        "no SSE events found in response body"
    );
    assert_eq!(
        event_types.first().copied(),
        Some("message_start"),
        "first event must be message_start"
    );
    assert_eq!(
        event_types.last().copied(),
        Some("message_stop"),
        "last event must be message_stop"
    );

    // At least one content_block_delta with text.
    let has_delta = event_types.iter().any(|&e| e == "content_block_delta");
    assert!(has_delta, "expected at least one content_block_delta event");

    // Parse the data lines for content_block_delta to confirm text is present.
    let mut found_text = false;
    let mut lines_iter = body.lines().peekable();
    while let Some(line) = lines_iter.next() {
        if line == "event: content_block_delta" {
            if let Some(data_line) = lines_iter.next() {
                if let Some(json_str) = data_line.strip_prefix("data: ") {
                    if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                        if val["delta"]["type"] == "text_delta"
                            && val["delta"]["text"].as_str().is_some()
                        {
                            found_text = true;
                            break;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_text,
        "no text_delta found in content_block_delta events"
    );
}

// ---------------------------------------------------------------------------
// c) Tool call
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_openai_tool_call() {
    let base = spawn_test_server(test_config()).await;
    let client = api_client();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 200,
            "tools": [{
                "name": "get_weather",
                "description": "Get the current weather in a given location.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "City and state, e.g. San Francisco, CA"
                        }
                    },
                    "required": ["location"]
                }
            }],
            "messages": [{
                "role": "user",
                "content": "What is the weather in San Francisco?"
            }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "expected 200, got {}", resp.status());

    let body: Value = resp.json().await.expect("response is not valid JSON");
    assert_eq!(body["type"], "message");

    // Find a tool_use content block.
    let content = body["content"].as_array().expect("content must be array");
    let tool_use = content
        .iter()
        .find(|b| b["type"] == "tool_use")
        .expect("expected a tool_use content block in response");

    let id = tool_use["id"].as_str().expect("tool_use must have id");
    assert!(!id.is_empty(), "tool_use id must not be empty");

    let name = tool_use["name"].as_str().expect("tool_use must have name");
    assert_eq!(name, "get_weather", "tool name must be get_weather");

    let input = &tool_use["input"];
    assert!(input.is_object(), "tool_use input must be an object");
    assert!(
        input["location"].as_str().is_some(),
        "tool input should contain 'location'"
    );
}

// ---------------------------------------------------------------------------
// d) Authentication error
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_openai_error() {
    // Build config with an invalid API key.
    let bad_key = "sk-invalid-key-for-testing";
    let config = Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: bad_key.to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o-mini".to_string(),
            small_model: "gpt-4o-mini".to_string(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken(bad_key.to_string()),
        log_bodies: false,
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    };

    let base = spawn_test_server(config).await;
    let client = api_client();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 10,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 401, "expected 401, got {}", resp.status());

    let body: Value = resp.json().await.expect("response is not valid JSON");
    assert_eq!(body["type"], "error", "response type must be 'error'");
    assert!(
        body["error"]["type"].as_str().is_some(),
        "error.type must be present"
    );
    assert!(
        body["error"]["message"].as_str().is_some(),
        "error.message must be present"
    );
}
