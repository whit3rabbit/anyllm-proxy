// Live integration tests for the Bedrock backend.
// Requires AWS credentials in the environment. Run with:
//   AWS_REGION=us-east-1 AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
//     cargo test --test live_bedrock -- --ignored --test-threads=1

use reqwest::Client;
use serde_json::json;

/// Start the proxy with Bedrock backend and return the base URL.
fn start_proxy() -> String {
    // These tests are run manually with real credentials.
    // The proxy must be started externally, or we construct a test server here.
    let port = std::env::var("TEST_PROXY_PORT").unwrap_or_else(|_| "3099".to_string());
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
#[ignore]
async fn bedrock_non_streaming() {
    let base = start_proxy();
    let client = Client::new();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "anthropic.claude-haiku-4-5-20251001-v1:0",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "Say hello in one word."}]
        }))
        .send()
        .await
        .expect("request failed");

    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.expect("invalid JSON response");

    assert_eq!(status, 200, "unexpected status: {body}");
    assert_eq!(body["type"], "message");
    assert!(body["content"].as_array().map_or(false, |a| !a.is_empty()));
}

#[tokio::test]
#[ignore]
async fn bedrock_streaming() {
    let base = start_proxy();
    let client = Client::new();

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "anthropic.claude-haiku-4-5-20251001-v1:0",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "Say hello."}]
        }))
        .send()
        .await
        .expect("request failed");

    let status = resp.status().as_u16();
    assert_eq!(status, 200, "expected 200 for streaming");

    let body = resp.text().await.expect("failed to read body");
    assert!(
        body.contains("message_start"),
        "expected message_start event in stream"
    );
    assert!(
        body.contains("message_stop"),
        "expected message_stop event in stream"
    );
}
