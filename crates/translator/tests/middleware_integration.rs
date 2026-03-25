//! Integration tests for the middleware module.
//! Uses a mock OpenAI backend (axum test server) to verify end-to-end translation.

#![cfg(feature = "middleware")]

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use tokio::net::TcpListener;

use anyllm_translate::middleware::{
    anthropic_compat_router, AnthropicCompatConfig, AnthropicTranslationLayer,
};
use anyllm_translate::TranslationConfig;

// --- Mock OpenAI backend ---

/// Mock handler that returns a canned ChatCompletionResponse.
async fn mock_chat_completion(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
    let model = req
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o");
    let is_stream = req.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    if is_stream {
        return mock_stream_response(model).await.into_response();
    }

    let response = serde_json::json!({
        "id": "chatcmpl-mock123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello from mock!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });

    (StatusCode::OK, Json(response)).into_response()
}

async fn mock_stream_response(
    model: &str,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let model = model.to_string();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(32);

    tokio::spawn(async move {
        // Chunk 1: role
        let chunk1 = serde_json::json!({
            "id": "chatcmpl-stream1",
            "object": "chat.completion.chunk",
            "created": 1700000000,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant"},
                "finish_reason": null
            }]
        });
        let _ = tx
            .send(Ok(
                Event::default().data(serde_json::to_string(&chunk1).unwrap())
            ))
            .await;

        // Chunk 2: text content
        let chunk2 = serde_json::json!({
            "id": "chatcmpl-stream1",
            "object": "chat.completion.chunk",
            "created": 1700000000,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": "Hello"},
                "finish_reason": null
            }]
        });
        let _ = tx
            .send(Ok(
                Event::default().data(serde_json::to_string(&chunk2).unwrap())
            ))
            .await;

        // Chunk 3: finish
        let chunk3 = serde_json::json!({
            "id": "chatcmpl-stream1",
            "object": "chat.completion.chunk",
            "created": 1700000000,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        });
        let _ = tx
            .send(Ok(
                Event::default().data(serde_json::to_string(&chunk3).unwrap())
            ))
            .await;

        // [DONE] sentinel
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
    });

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

/// Mock handler that returns a 429 rate limit error.
async fn mock_rate_limit() -> impl IntoResponse {
    let body = serde_json::json!({
        "error": {
            "message": "Rate limit exceeded",
            "type": "rate_limit_error",
            "code": "rate_limit_exceeded"
        }
    });
    (StatusCode::TOO_MANY_REQUESTS, Json(body))
}

/// Spin up a mock server and return its base URL.
async fn start_mock_server(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn test_config(backend_url: &str) -> AnthropicCompatConfig {
    AnthropicCompatConfig::builder()
        .backend_url(backend_url)
        .api_key("test-key")
        .translation(
            TranslationConfig::builder()
                .model_map("haiku", "gpt-4o-mini")
                .model_map("sonnet", "gpt-4o")
                .build(),
        )
        .build()
}

// --- Tests ---

#[tokio::test]
async fn non_streaming_request_translates_roundtrip() {
    let mock = Router::new().route("/v1/chat/completions", post(mock_chat_completion));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);
    let app = anthropic_compat_router(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    // Response should be in Anthropic format
    assert_eq!(body["type"], "message");
    assert_eq!(body["model"], "claude-sonnet-4-6"); // original model preserved
    assert_eq!(body["role"], "assistant");
    assert_eq!(body["content"][0]["type"], "text");
    assert_eq!(body["content"][0]["text"], "Hello from mock!");
    assert_eq!(body["usage"]["input_tokens"], 10);
    assert_eq!(body["usage"]["output_tokens"], 5);
    assert_eq!(body["stop_reason"], "end_turn");
}

#[tokio::test]
async fn streaming_request_produces_anthropic_sse_events() {
    let mock = Router::new().route("/v1/chat/completions", post(mock_chat_completion));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);
    let app = anthropic_compat_router(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "stream": true,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Collect SSE events from the response body
    let body = resp.text().await.unwrap();

    // Should contain Anthropic event types
    assert!(
        body.contains("event: message_start"),
        "missing message_start: {body}"
    );
    assert!(
        body.contains("event: content_block_start"),
        "missing content_block_start: {body}"
    );
    assert!(
        body.contains("event: content_block_delta"),
        "missing content_block_delta: {body}"
    );
    assert!(
        body.contains("event: message_stop"),
        "missing message_stop: {body}"
    );

    // The text delta should contain "Hello"
    assert!(body.contains("Hello"), "missing text content: {body}");
}

#[tokio::test]
async fn backend_error_translated_to_anthropic_format() {
    let mock = Router::new().route("/v1/chat/completions", post(mock_rate_limit));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);
    let app = anthropic_compat_router(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 429);
    let body: serde_json::Value = resp.json().await.unwrap();

    // Should be Anthropic error format
    assert_eq!(body["type"], "error");
    assert!(body["error"]["type"].as_str().is_some());
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Rate limit"));
}

#[tokio::test]
async fn tower_layer_intercepts_messages_passes_through_others() {
    let mock = Router::new().route("/v1/chat/completions", post(mock_chat_completion));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);

    // An existing service with a custom route
    let app = Router::new()
        .route(
            "/custom",
            axum::routing::get(|| async { "custom response" }),
        )
        .layer(AnthropicTranslationLayer::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Custom route still works
    let resp = client
        .get(format!("http://{addr}/custom"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "custom response");

    // /v1/messages is intercepted by the layer
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "message");
}

#[tokio::test]
async fn invalid_json_returns_anthropic_error() {
    let mock = Router::new().route("/v1/chat/completions", post(mock_chat_completion));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);
    let app = anthropic_compat_router(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    // Should get an error response (400 or 422 depending on axum's JSON extractor)
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn model_mapping_applied() {
    // Mock that echoes the model it received
    async fn echo_model(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
        let model = req
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let response = serde_json::json!({
            "id": "chatcmpl-echo",
            "object": "chat.completion",
            "created": 1700000000,
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": format!("model={model}")},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        });
        Json(response)
    }

    let mock = Router::new().route("/v1/chat/completions", post(echo_model));
    let base_url = start_mock_server(mock).await;

    let config = test_config(&base_url);
    let app = anthropic_compat_router(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .json(&serde_json::json!({
            "model": "claude-haiku-3",
            "max_tokens": 10,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // The mock echoes the model it received; should be the mapped model
    let text = body["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("gpt-4o-mini"),
        "expected mapped model in: {text}"
    );
    // But the Anthropic response should have the original model name
    assert_eq!(body["model"], "claude-haiku-3");
}
