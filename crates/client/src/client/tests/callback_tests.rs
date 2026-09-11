//! Tests for the `success_callback` hook on `anyllm_client::Client`.
//!
//! Each test stands up a small `tokio::net::TcpListener` that returns a canned
//! HTTP/1.1 JSON response (the OpenAI `chat.completion` shape), then drives the
//! client through `Client::messages` and asserts what the callback observed.
//!
//! The hook must fire exactly once per successful `messages()` call, with the
//! original Anthropic request, the translated Anthropic response, the resolved
//! backend model, and (when configured) a computed `cost_usd`.

use super::*;
use crate::callback::{CallbackInput, PricingConfig, SuccessCallback};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// OpenAI-style chat.completion JSON response (the shape `translate_response` reads).
fn canned_openai_response(backend_model: &str, input_tokens: u64, output_tokens: u64) -> String {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_700_000_000,
        "model": backend_model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
    .to_string()
}

/// Minimal HTTP/1.1 server: handles any number of requests, each replying
/// with the same `body`. We do NOT parse the inbound request — reqwest sends
/// headers and body together over a keep-alive connection; we just write the
/// canned response and let the client drain it.
async fn spawn_canned_backend(body: String, status: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        }
    });
    format!("http://{addr}/v1/chat/completions")
}

fn anthropic_request() -> anyllm_translate::anthropic::MessageCreateRequest {
    serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "hi"}]
    }))
    .unwrap()
}

type Captured = Arc<Mutex<Vec<CallbackInput>>>;

fn recording_callback() -> (SuccessCallback, Captured, Arc<AtomicUsize>) {
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let count = Arc::new(AtomicUsize::new(0));
    let cb_captured = Arc::clone(&captured);
    let cb_count = Arc::clone(&count);
    let cb: SuccessCallback = Arc::new(move |input: CallbackInput| {
        cb_count.fetch_add(1, Ordering::SeqCst);
        cb_captured.lock().unwrap().push(input);
    });
    (cb, captured, count)
}

fn build_test_client(
    base_url: String,
    success_callback: Option<SuccessCallback>,
    pricing: Option<PricingConfig>,
) -> Client {
    let builder = ClientConfigBuilder::default()
        .backend_url(base_url)
        .auth(Auth::Bearer("sk-test".into()))
        .http(HttpClientConfig {
            ssrf_protection: false,
            // Tight timeouts so a misbehaving test backend can't hang the suite.
            read_timeout: Some(std::time::Duration::from_secs(5)),
            connect_timeout: Some(std::time::Duration::from_secs(2)),
            ..HttpClientConfig::new()
        });
    let builder = if let Some(cb) = success_callback {
        builder.success_callback(cb)
    } else {
        builder
    };
    let builder = if let Some(p) = pricing {
        builder.pricing(p)
    } else {
        builder
    };
    let config = builder.build();
    Client::new(config)
}

#[tokio::test]
async fn callback_fires_with_input_and_response() {
    let url = spawn_canned_backend(canned_openai_response("claude-sonnet-4-6", 12, 7), 200).await;
    let (cb, captured, count) = recording_callback();
    let client = build_test_client(url, Some(cb), None);

    let req = anthropic_request();
    let resp = client
        .messages(&req)
        .await
        .expect("messages should succeed");

    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "callback fires exactly once"
    );
    let inputs = captured.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    let input = &inputs[0];
    // Without `model_map`, the Anthropic request model name flows through to
    // the backend unchanged (after translation). The backend reports the same
    // model name back in the response payload.
    assert_eq!(input.request_model, "claude-sonnet-4-6");
    assert_eq!(input.backend_model, "claude-sonnet-4-6");
    assert_eq!(input.response.usage.input_tokens, 12);
    assert_eq!(input.response.usage.output_tokens, 7);
    assert!(input.duration.as_nanos() > 0, "duration is measured");
    assert!(input.cost_usd.is_none(), "no pricing => cost_usd is None");
    // Sanity-check the response is the real translated response.
    assert_eq!(resp.usage.input_tokens, 12);
    assert_eq!(resp.usage.output_tokens, 7);
}

#[tokio::test]
async fn callback_not_called_on_transport_error() {
    // Bind a listener that accepts the connection but never writes a response,
    // so reqwest surfaces a transport error (read timeout / connection drop).
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            // Read the request, then close without writing a response.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            drop(stream);
        }
    });
    let url = format!("http://{addr}/v1/chat/completions");

    let (cb, _captured, count) = recording_callback();
    let client = build_test_client(url, Some(cb), None);

    let req = anthropic_request();
    let result = client.messages(&req).await;

    assert!(result.is_err(), "transport error must surface");
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "callback MUST NOT fire on error"
    );
}

#[tokio::test]
async fn callback_not_called_on_api_error_status() {
    let url = spawn_canned_backend(r#"{"error":"bad request"}"#.to_string(), 400).await;
    let (cb, _captured, count) = recording_callback();
    let client = build_test_client(url, Some(cb), None);

    let req = anthropic_request();
    let result = client.messages(&req).await;

    assert!(result.is_err(), "4xx must surface");
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "callback MUST NOT fire on API error"
    );
}

#[tokio::test]
async fn callback_panic_does_not_break_call() {
    let url = spawn_canned_backend(canned_openai_response("gpt-4o-mini", 5, 3), 200).await;
    let cb: SuccessCallback = Arc::new(|_input: CallbackInput| {
        panic!("user-supplied callback exploded");
    });
    let client = build_test_client(url, Some(cb), None);

    let req = anthropic_request();
    let resp = client
        .messages(&req)
        .await
        .expect("callback panic must NOT propagate; SDK returns the response");

    assert_eq!(resp.usage.input_tokens, 5);
}

#[tokio::test]
async fn callback_cost_usd_uses_pricing_map() {
    let url = spawn_canned_backend(canned_openai_response("gpt-4o", 1_000_000, 500_000), 200).await;
    let (cb, captured, _count) = recording_callback();
    let pricing = PricingConfig::new()
        .with_price("gpt-4o", 2.50, 10.00) // $2.50 / 1M in, $10.00 / 1M out
        .with_price("gpt-4o-mini", 0.15, 0.60);
    // Use model_map so the backend model name sent to the canned backend
    // (and used for cost lookup) is "gpt-4o" instead of the request's
    // "claude-sonnet-4-6".
    let translation = anyllm_translate::TranslationConfig::builder()
        .model_map("sonnet", "gpt-4o")
        .build();
    let builder = ClientConfigBuilder::default()
        .backend_url(url)
        .auth(Auth::Bearer("sk-test".into()))
        .http(HttpClientConfig {
            ssrf_protection: false,
            ..HttpClientConfig::new()
        })
        .translation(translation)
        .success_callback(cb)
        .pricing(pricing);
    let config = builder.build();
    let client = Client::new(config);

    let mut req = anthropic_request();
    req.model = "claude-3-sonnet".into(); // matches the model_map key
    let _ = client
        .messages(&req)
        .await
        .expect("messages should succeed");

    let inputs = captured.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    let cost = inputs[0]
        .cost_usd
        .expect("pricing was configured => cost_usd must be Some");
    assert_eq!(inputs[0].backend_model, "gpt-4o", "model_map applied");
    // 1_000_000 * (2.50 / 1_000_000) = 2.50 input
    // 500_000 * (10.00 / 1_000_000) = 5.00 output
    // total = 7.50 USD
    let expected = 2.50 + 5.00;
    assert!(
        (cost - expected).abs() < 1e-9,
        "expected cost {expected}, got {cost}"
    );
}

#[tokio::test]
async fn callback_cost_usd_none_when_model_unknown() {
    let url =
        spawn_canned_backend(canned_openai_response("some-unknown-model", 100, 50), 200).await;
    let (cb, captured, _count) = recording_callback();
    let pricing = PricingConfig::new().with_price("gpt-4o", 2.50, 10.00);
    let client = build_test_client(url, Some(cb), Some(pricing));

    let req = anthropic_request();
    let _ = client
        .messages(&req)
        .await
        .expect("messages should succeed");

    let inputs = captured.lock().unwrap();
    assert!(
        inputs[0].cost_usd.is_none(),
        "unknown model => cost_usd None"
    );
}

#[tokio::test]
async fn callback_cost_usd_none_when_pricing_not_configured() {
    let url = spawn_canned_backend(canned_openai_response("gpt-4o", 100, 50), 200).await;
    let (cb, captured, _count) = recording_callback();
    let client = build_test_client(url, Some(cb), None);

    let req = anthropic_request();
    let _ = client
        .messages(&req)
        .await
        .expect("messages should succeed");

    let inputs = captured.lock().unwrap();
    assert!(
        inputs[0].cost_usd.is_none(),
        "no pricing configured => cost_usd None"
    );
}

#[tokio::test]
async fn callback_is_shared_across_cloned_clients() {
    let url = spawn_canned_backend(canned_openai_response("gpt-4o-mini", 1, 1), 200).await;
    let (cb, captured, count) = recording_callback();
    let client = build_test_client(url, Some(cb), None);
    let client2 = client.clone();

    let req = anthropic_request();
    client.messages(&req).await.expect("first call ok");
    client2.messages(&req).await.expect("second call ok");

    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "cloned clients share the callback"
    );
    assert_eq!(captured.lock().unwrap().len(), 2);
}

// Sanity: a request with no callback configured still succeeds and the SDK is
// observably indistinguishable from before this feature landed.
#[tokio::test]
async fn no_callback_still_works() {
    let url = spawn_canned_backend(canned_openai_response("gpt-4o-mini", 2, 2), 200).await;
    let client = build_test_client(url, None, None);

    let req = anthropic_request();
    let resp = client
        .messages(&req)
        .await
        .expect("messages should succeed");
    assert_eq!(resp.usage.input_tokens, 2);
    assert_eq!(resp.usage.output_tokens, 2);
}

// Compile-time witness: `CallbackInput`, `SuccessCallback`, `PricingConfig`,
// `ClientConfigBuilder::success_callback`, and `ClientConfigBuilder::pricing`
// are all publicly reachable from the crate root. If any of these names change,
// the test crate fails to build before runtime tests even run.
#[test]
fn public_api_surface_compiles() {
    fn _accepts(_: CallbackInput) {}
    fn _accepts_cb(_: SuccessCallback) {}
    fn _accepts_pricing(_: PricingConfig) {}
    let _p: PricingConfig = PricingConfig::new();
}
