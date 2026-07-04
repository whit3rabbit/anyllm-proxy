// Integration tests for Anthropic thinking-block record-and-restore repair
// (ANTHROPIC_THINKING_REPAIR=true, BACKEND=anthropic passthrough only).
//
// Drives the real /v1/messages route (`anthropic_passthrough`) against a
// mock upstream, covering: non-streaming record -> non-streaming repair,
// and streaming record -> non-streaming repair (same store, both paths).

use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{response::IntoResponse, routing::post, Router};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::net::TcpListener;

fn anthropic_config_with_base(base_url: &str) -> Config {
    anthropic_config_with_base_and_repair(base_url, true)
}

fn anthropic_config_with_base_and_repair(
    base_url: &str,
    anthropic_thinking_repair: bool,
) -> Config {
    Config {
        backend: BackendKind::Anthropic,
        openai_api_key: "anthropic-backend-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("anthropic-backend-key".into()),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    }
}

async fn spawn_proxy_with_thinking_repair(config: Config) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    // ANTHROPIC_THINKING_REPAIR is no longer read directly by routes.rs (the
    // store is always constructed; only `Config.anthropic_thinking_repair`,
    // set directly on the `Config` literal, gates behavior via
    // RuntimeConfig). Left set here for defense-in-depth / documentation.
    std::env::set_var("ANTHROPIC_THINKING_REPAIR", "true");
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Mock upstream Anthropic API: first call returns a fixed non-streaming
/// response (the "ground truth" the repair store will record); every call
/// after that echoes the request body it received back to the test via
/// `received`, and acks with a minimal valid response.
async fn spawn_mock_anthropic_backend(
    first_response: Value,
    received: Arc<Mutex<Vec<Value>>>,
) -> String {
    let call_count = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/v1/messages",
        post(move |body: axum::body::Bytes| {
            let call_count = call_count.clone();
            let first_response = first_response.clone();
            let received = received.clone();
            async move {
                let n = call_count.fetch_add(1, Ordering::SeqCst);
                let parsed: Value = serde_json::from_slice(&body).unwrap();
                received.lock().unwrap().push(parsed);
                if n == 0 {
                    axum::Json(first_response).into_response()
                } else {
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
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn thinking_block(text: &str, sig: &str) -> Value {
    json!({"type": "thinking", "thinking": text, "signature": sig})
}

fn tool_use_block(id: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": "get_weather", "input": {"city": "nyc"}})
}

fn ground_truth_response() -> Value {
    json!({
        "id": "msg_ground_truth",
        "type": "message",
        "role": "assistant",
        "content": [thinking_block("the correct, original thought", "sig_real"), tool_use_block("toolu_1")],
        "model": "claude-opus-4-5",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    })
}

fn corrupted_replay_request() -> Value {
    json!({
        "model": "claude-opus-4-5",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "what's the weather in nyc?"},
            {
                "role": "assistant",
                "content": [
                    thinking_block("garbled merged corrupted text", "sig_real"),
                    tool_use_block("toolu_1"),
                ],
            },
            {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "72F sunny"}],
            },
        ]
    })
}

#[tokio::test]
async fn non_streaming_record_then_repair_restores_mutated_thinking_text() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(ground_truth_response(), received.clone()).await;
    let proxy = spawn_proxy_with_thinking_repair(anthropic_config_with_base(&mock)).await;
    let client = Client::new();

    // First turn: proxy forwards untouched (no history yet), records the
    // response's thinking block + tool_use ownership as ground truth.
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "what's the weather in nyc?"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Second turn: client replays history with the assistant's thinking
    // text corrupted (signature intact). Proxy should restore the original
    // bytes before forwarding upstream.
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&corrupted_replay_request())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(calls.len(), 2, "expected exactly 2 upstream calls");
    let forwarded = &calls[1];
    let forwarded_thinking = forwarded["messages"][1]["content"][0]["thinking"]
        .as_str()
        .unwrap();
    assert_eq!(
        forwarded_thinking, "the correct, original thought",
        "repair should have restored the recorded original text before forwarding upstream"
    );
}

async fn spawn_mock_anthropic_streaming_then_echo_backend(
    received: Arc<Mutex<Vec<Value>>>,
) -> String {
    let call_count = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/v1/messages",
        post(move |body: axum::body::Bytes| {
            let call_count = call_count.clone();
            let received = received.clone();
            async move {
                let n = call_count.fetch_add(1, Ordering::SeqCst);
                let parsed: Value = serde_json::from_slice(&body).unwrap();
                received.lock().unwrap().push(parsed);
                if n == 0 {
                    let sse = concat!(
                        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream_gt\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-4-5\",\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"streamed original thought\"}}\n\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_stream\"}}\n\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_stream_1\",\"name\":\"get_weather\",\"input\":{}}}\n\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"nyc\\\"}\"}}\n\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":8}}\n\n",
                        "data: {\"type\":\"message_stop\"}\n\n",
                    );
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        sse,
                    )
                        .into_response()
                } else {
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
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn streaming_record_feeds_non_streaming_repair_on_next_turn() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_streaming_then_echo_backend(received.clone()).await;
    let proxy = spawn_proxy_with_thinking_repair(anthropic_config_with_base(&mock)).await;
    let client = Client::new();

    // First turn: streamed response. The proxy must record its thinking
    // block + tool_use ownership from the SSE frames as they pass through.
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{"role": "user", "content": "what's the weather in nyc?"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // Drain the SSE body so the background recording task completes.
    let _ = resp.bytes().await.unwrap();

    // Give the spawned recording task a moment to commit (it runs after the
    // response stream finishes, on a separate tokio task from this request).
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Second turn (non-streaming): replay with mutated thinking text under
    // the signature recorded from the stream.
    let corrupted = json!({
        "model": "claude-opus-4-5",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "what's the weather in nyc?"},
            {
                "role": "assistant",
                "content": [
                    thinking_block("mutated during replay", "sig_stream"),
                    json!({"type": "tool_use", "id": "toolu_stream_1", "name": "get_weather", "input": {"city": "nyc"}}),
                ],
            },
            {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "toolu_stream_1", "content": "72F sunny"}],
            },
        ]
    });
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&corrupted)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(calls.len(), 2, "expected exactly 2 upstream calls");
    let forwarded_thinking = calls[1]["messages"][1]["content"][0]["thinking"]
        .as_str()
        .unwrap();
    assert_eq!(
        forwarded_thinking, "streamed original thought",
        "repair should restore the text recorded from the streamed response"
    );
}

/// Proves the live `anthropic_thinking_repair` flag -- not just store
/// construction -- gates behavior: the store is always constructed for
/// Anthropic backends now, so if disabling only skipped construction (and
/// the use sites weren't actually gated), this would still repair.
#[tokio::test]
async fn anthropic_thinking_repair_disabled_forwards_corrupted_text_unrepaired() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(ground_truth_response(), received.clone()).await;
    let proxy =
        spawn_proxy_with_thinking_repair(anthropic_config_with_base_and_repair(&mock, false)).await;
    let client = Client::new();

    // First turn: would record ground truth if enabled; disabled, so it must not.
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "what's the weather in nyc?"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Second turn: same corrupted replay as the enabled test. With repair
    // disabled, the corrupted text must reach upstream untouched.
    let resp = client
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&corrupted_replay_request())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let calls = received.lock().unwrap();
    assert_eq!(calls.len(), 2, "expected exactly 2 upstream calls");
    let forwarded_thinking = calls[1]["messages"][1]["content"][0]["thinking"]
        .as_str()
        .unwrap();
    assert_eq!(
        forwarded_thinking, "garbled merged corrupted text",
        "repair must be a no-op when anthropic_thinking_repair is disabled"
    );
}
