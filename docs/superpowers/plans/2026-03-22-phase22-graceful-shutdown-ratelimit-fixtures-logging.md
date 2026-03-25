# Phase 22: Graceful Shutdown, Rate Limits, Error Fixtures, Logging Toggle

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add graceful shutdown with in-flight request draining, rate limit header passthrough, error/edge case test fixtures, and opt-in request/response body logging.

**Architecture:** Four independent features added to the existing proxy crate. Graceful shutdown installs a tokio signal handler and uses axum's `with_graceful_shutdown`. Rate limit headers are captured from the backend response and injected into the Anthropic response via a new middleware. Error fixtures add golden-file tests for 4xx/5xx/malformed scenarios. Logging toggle adds a config flag and middleware that logs redacted request/response bodies.

**Tech Stack:** tokio (signal), axum (graceful shutdown, middleware), reqwest (response headers), serde_json (fixtures)

---

### Task 1: Graceful Shutdown with In-Flight Draining

**Files:**
- Modify: `crates/proxy/src/main.rs`
- Create: `crates/proxy/tests/shutdown.rs`

Race condition note: `tokio::signal::ctrl_c()` is edge-triggered. We use `tokio::select!` between the server future and the signal future. The axum `with_graceful_shutdown` API handles the draining: once the shutdown signal fires, it stops accepting new connections but lets in-flight requests complete. No custom synchronization needed; axum + hyper handle the drain internally. The key correctness property: spawned streaming tasks (in `messages_stream`) hold a `tx` sender; the SSE response completes when `tx` is dropped, so in-flight streams drain naturally.

- [ ] **Step 1: Write the integration test for graceful shutdown**

Create `crates/proxy/tests/shutdown.rs`:

```rust
// Test: server shuts down cleanly on signal, in-flight requests complete.

use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;
use std::time::Duration;

fn test_config() -> Config {
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
    }
}

#[tokio::test]
async fn server_shuts_down_cleanly() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Verify server is up
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Send shutdown signal
    shutdown_tx.send(()).unwrap();

    // Server task should complete within a reasonable time
    let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server did not shut down within 5 seconds");
    assert!(result.unwrap().is_ok(), "server task panicked");
}

#[tokio::test]
async fn in_flight_health_completes_during_shutdown() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Start a request
    let health_url = format!("http://{addr}/health");
    let resp = reqwest::get(&health_url).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Signal shutdown then immediately verify another request still works
    // (the server drains, so already-accepted connections complete)
    shutdown_tx.send(()).unwrap();

    // Server should finish
    let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server did not shut down within 5 seconds");
}

#[tokio::test]
async fn new_connections_refused_after_shutdown() {
    let app = routes::app(test_config());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Verify server is up
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Shut down and wait for server to exit
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .unwrap()
        .unwrap();

    // New connection should fail (connection refused)
    let result = reqwest::get(format!("http://{addr}/health")).await;
    assert!(result.is_err(), "expected connection refused after shutdown");
}
```

- [ ] **Step 2: Run tests to verify they pass (validates the axum primitive)**

Run: `cargo test -p anyllm_proxy --test shutdown -- --nocapture 2>&1`
Expected: All 3 tests pass. These test the `axum::serve(...).with_graceful_shutdown()` API directly, not our `main.rs`. This validates the mechanism before we wire it into production code. The `main.rs` change (Step 3) applies the same pattern to the real server.

- [ ] **Step 3: Update main.rs to use graceful shutdown**

Replace `crates/proxy/src/main.rs`:

```rust
use anyllm_proxy::{config, server::routes};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = config::Config::from_env();
    let listen_port = config.listen_port;
    let app = routes::app(config);

    let addr = format!("0.0.0.0:{listen_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {addr}: {e}"));

    tracing::info!("proxy listening on {addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    tracing::info!("server shut down gracefully");
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => { tracing::info!("received SIGINT, starting graceful shutdown"); }
        _ = sigterm.recv() => { tracing::info!("received SIGTERM, starting graceful shutdown"); }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for Ctrl+C");
        tracing::info!("received Ctrl+C, starting graceful shutdown");
    }
}
```

- [ ] **Step 4: Run all tests to verify nothing broke**

Run: `cargo test -p anyllm_proxy 2>&1`
Expected: All tests pass (existing + new shutdown tests).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: Clean.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/main.rs crates/proxy/tests/shutdown.rs
git commit -m "feat: add graceful shutdown with SIGINT/SIGTERM and in-flight draining (Phase 22)"
```

---

### Task 2: Rate Limit Header Passthrough

**Files:**
- Modify: `crates/proxy/src/backend/mod.rs`
- Modify: `crates/proxy/src/backend/openai_client.rs`
- Modify: `crates/proxy/src/backend/gemini_client.rs`
- Modify: `crates/proxy/src/server/routes.rs`

Design: Backend clients currently discard response headers after retry logic. We need to capture rate limit headers from successful backend responses and inject them into the Anthropic response. OpenAI sends `x-ratelimit-limit-requests`, `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-requests`, `x-ratelimit-remaining-tokens`, `x-ratelimit-reset-requests`, `x-ratelimit-reset-tokens`. Anthropic uses `anthropic-ratelimit-requests-limit`, `anthropic-ratelimit-requests-remaining`, `anthropic-ratelimit-requests-reset`, `anthropic-ratelimit-tokens-limit`, `anthropic-ratelimit-tokens-remaining`, `anthropic-ratelimit-tokens-reset`, plus `retry-after`. We map the OpenAI headers to their Anthropic equivalents. Gemini does not send rate limit headers, so Gemini responses get no rate limit headers.

- [ ] **Step 1: Define the rate limit header struct**

Add to `crates/proxy/src/backend/mod.rs`:

```rust
/// Rate limit headers captured from backend responses.
/// Maps between OpenAI x-ratelimit-* and Anthropic anthropic-ratelimit-* headers.
#[derive(Debug, Default, Clone)]
pub struct RateLimitHeaders {
    pub requests_limit: Option<String>,
    pub requests_remaining: Option<String>,
    pub requests_reset: Option<String>,
    pub tokens_limit: Option<String>,
    pub tokens_remaining: Option<String>,
    pub tokens_reset: Option<String>,
    pub retry_after: Option<String>,
}

impl RateLimitHeaders {
    /// Extract rate limit headers from an OpenAI response.
    pub fn from_openai_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            requests_limit: header_str(headers, "x-ratelimit-limit-requests"),
            requests_remaining: header_str(headers, "x-ratelimit-remaining-requests"),
            requests_reset: header_str(headers, "x-ratelimit-reset-requests"),
            tokens_limit: header_str(headers, "x-ratelimit-limit-tokens"),
            tokens_remaining: header_str(headers, "x-ratelimit-remaining-tokens"),
            tokens_reset: header_str(headers, "x-ratelimit-reset-tokens"),
            retry_after: header_str(headers, "retry-after"),
        }
    }

    /// Inject as Anthropic-format response headers.
    pub fn inject_anthropic_headers(&self, headers: &mut axum::http::HeaderMap) {
        set_if_some(headers, "anthropic-ratelimit-requests-limit", &self.requests_limit);
        set_if_some(headers, "anthropic-ratelimit-requests-remaining", &self.requests_remaining);
        set_if_some(headers, "anthropic-ratelimit-requests-reset", &self.requests_reset);
        set_if_some(headers, "anthropic-ratelimit-tokens-limit", &self.tokens_limit);
        set_if_some(headers, "anthropic-ratelimit-tokens-remaining", &self.tokens_remaining);
        set_if_some(headers, "anthropic-ratelimit-tokens-reset", &self.tokens_reset);
        set_if_some(headers, "retry-after", &self.retry_after);
    }
}

fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(String::from)
}

fn set_if_some(headers: &mut axum::http::HeaderMap, name: &str, value: &Option<String>) {
    if let Some(v) = value {
        if let Ok(hv) = v.parse() {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                hv,
            );
        }
    }
}
```

- [ ] **Step 2: Write unit tests for RateLimitHeaders**

Add to `crates/proxy/src/backend/mod.rs` test module (or create new test block):

```rust
#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    #[test]
    fn from_openai_headers_extracts_all() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", "100".parse().unwrap());
        headers.insert("x-ratelimit-remaining-requests", "99".parse().unwrap());
        headers.insert("x-ratelimit-reset-requests", "1s".parse().unwrap());
        headers.insert("x-ratelimit-limit-tokens", "40000".parse().unwrap());
        headers.insert("x-ratelimit-remaining-tokens", "39500".parse().unwrap());
        headers.insert("x-ratelimit-reset-tokens", "500ms".parse().unwrap());
        headers.insert("retry-after", "1".parse().unwrap());

        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert_eq!(rl.requests_limit.as_deref(), Some("100"));
        assert_eq!(rl.requests_remaining.as_deref(), Some("99"));
        assert_eq!(rl.requests_reset.as_deref(), Some("1s"));
        assert_eq!(rl.tokens_limit.as_deref(), Some("40000"));
        assert_eq!(rl.tokens_remaining.as_deref(), Some("39500"));
        assert_eq!(rl.tokens_reset.as_deref(), Some("500ms"));
        assert_eq!(rl.retry_after.as_deref(), Some("1"));
    }

    #[test]
    fn from_openai_headers_missing_are_none() {
        let headers = reqwest::header::HeaderMap::new();
        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert!(rl.requests_limit.is_none());
        assert!(rl.tokens_limit.is_none());
        assert!(rl.retry_after.is_none());
    }

    #[test]
    fn inject_anthropic_headers_sets_values() {
        let rl = RateLimitHeaders {
            requests_limit: Some("100".into()),
            requests_remaining: Some("99".into()),
            tokens_limit: Some("40000".into()),
            ..Default::default()
        };
        let mut headers = axum::http::HeaderMap::new();
        rl.inject_anthropic_headers(&mut headers);

        assert_eq!(headers.get("anthropic-ratelimit-requests-limit").unwrap(), "100");
        assert_eq!(headers.get("anthropic-ratelimit-requests-remaining").unwrap(), "99");
        assert_eq!(headers.get("anthropic-ratelimit-tokens-limit").unwrap(), "40000");
        // Not set: should be absent
        assert!(headers.get("anthropic-ratelimit-tokens-remaining").is_none());
        assert!(headers.get("retry-after").is_none());
    }

    #[test]
    fn inject_anthropic_headers_empty_is_noop() {
        let rl = RateLimitHeaders::default();
        let mut headers = axum::http::HeaderMap::new();
        rl.inject_anthropic_headers(&mut headers);
        assert!(headers.is_empty());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail (struct doesn't exist yet)**

Run: `cargo test -p anyllm_proxy rate_limit 2>&1`
Expected: Compilation error.

- [ ] **Step 4: Implement `RateLimitHeaders` in mod.rs**

Add the struct, `from_openai_headers`, `inject_anthropic_headers`, and helper functions from Step 1 to `crates/proxy/src/backend/mod.rs`.

- [ ] **Step 5: Run unit tests**

Run: `cargo test -p anyllm_proxy rate_limit 2>&1`
Expected: All 4 rate_limit tests pass.

- [ ] **Step 6: Modify OpenAI client to return rate limit headers**

In `crates/proxy/src/backend/openai_client.rs`, change `chat_completion` to capture headers from the response before deserializing:

```rust
pub async fn chat_completion(
    &self,
    req: &openai::ChatCompletionRequest,
) -> Result<(openai::ChatCompletionResponse, u16, super::RateLimitHeaders), OpenAIClientError> {
    let response = self.send_with_retry(req).await?;
    let status = response.status().as_u16();
    let rate_limits = super::RateLimitHeaders::from_openai_headers(response.headers());
    let body = response
        .json::<openai::ChatCompletionResponse>()
        .await
        .map_err(OpenAIClientError::Deserialization)?;
    Ok((body, status, rate_limits))
}
```

Also update `chat_completion_stream` to return `RateLimitHeaders`:

```rust
pub async fn chat_completion_stream(
    &self,
    req: &openai::ChatCompletionRequest,
) -> Result<(reqwest::Response, super::RateLimitHeaders), OpenAIClientError> {
    let response = self.send_with_retry(req).await?;
    let rate_limits = super::RateLimitHeaders::from_openai_headers(response.headers());
    Ok((response, rate_limits))
}
```

- [ ] **Step 7: Update Gemini client for consistent signature**

In `crates/proxy/src/backend/gemini_client.rs`, update both methods to also return `RateLimitHeaders` (always default/empty, since Gemini doesn't send them):

```rust
pub async fn generate_content(
    &self,
    req: &gemini::GenerateContentRequest,
    model: &str,
) -> Result<(gemini::GenerateContentResponse, u16, super::RateLimitHeaders), GeminiClientError> {
    let url = self.generate_content_url(model);
    let response = self.send_with_retry(req, &url).await?;
    let status = response.status().as_u16();
    let rate_limits = super::RateLimitHeaders::default();
    let body = response
        .json::<gemini::GenerateContentResponse>()
        .await
        .map_err(GeminiClientError::Deserialization)?;
    Ok((body, status, rate_limits))
}

pub async fn generate_content_stream(
    &self,
    req: &gemini::GenerateContentRequest,
    model: &str,
) -> Result<(reqwest::Response, super::RateLimitHeaders), GeminiClientError> {
    let url = self.stream_generate_content_url(model);
    let response = self.send_with_retry(req, &url).await?;
    let rate_limits = super::RateLimitHeaders::default();
    Ok((response, rate_limits))
}
```

- [ ] **Step 8: Update routes.rs to propagate rate limit headers**

In `crates/proxy/src/server/routes.rs`, update the `messages` handler to inject headers into responses:

For non-streaming OpenAI/Vertex:
```rust
match client.chat_completion(&openai_req).await {
    Ok((openai_resp, _status, rate_limits)) => {
        state.metrics.record_success();
        let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
            &openai_resp,
            &original_model,
        );
        let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
        rate_limits.inject_anthropic_headers(response.headers_mut());
        response
    }
    // ...
}
```

For non-streaming Gemini:
```rust
match client.generate_content(&gemini_req, &mapped_model).await {
    Ok((gemini_resp, _status, rate_limits)) => {
        state.metrics.record_success();
        let anthropic_resp = mapping::gemini_message_map::gemini_to_anthropic_response(
            &gemini_resp,
            &original_model,
        );
        let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
        rate_limits.inject_anthropic_headers(response.headers_mut());
        response
    }
    // ...
}
```

For streaming, rate limit headers arrive with the initial SSE connection. Store them in the `AppState`-adjacent data or inject them into the SSE response headers. Since `messages_stream` returns `Sse<...>`, convert it to a `Response` and inject:

In `messages_stream`, return `(RateLimitHeaders, Sse<...>)` and handle in `messages`:
```rust
if body.stream == Some(true) {
    let (rate_limits, sse) = messages_stream(state, body).await;
    let mut response = sse.into_response();
    rate_limits.inject_anthropic_headers(response.headers_mut());
    return response;
}
```

Update `messages_stream` signature to return `(RateLimitHeaders, Sse<...>)`. Use a oneshot channel from the spawned task to send rate limit headers back to the caller. Both the OpenAI/Vertex and Gemini branches must send on `rl_tx`.

**Behavioral note:** This changes streaming behavior: the SSE response is now delayed until the backend connection succeeds (or fails). This is necessary because HTTP response headers must be sent before the body. Previously the SSE response started immediately and the spawned task populated it asynchronously, but rate limit headers could not be injected that way.

```rust
use crate::backend::RateLimitHeaders;

async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
) -> (RateLimitHeaders, Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>) {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);
    let (rl_tx, rl_rx) = tokio::sync::oneshot::channel::<RateLimitHeaders>();

    let metrics = state.metrics.clone();

    match &state.backend {
        BackendClient::OpenAI(client) | BackendClient::Vertex(client) => {
            let client = client.clone();
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            openai_req.model = state.model_mapping.map_model(&openai_req.model);
            let model = body.model.clone();

            tokio::spawn(async move {
                match client.chat_completion_stream(&openai_req).await {
                    Ok((response, rate_limits)) => {
                        // Send rate limits immediately, before streaming data
                        rl_tx.send(rate_limits).ok();
                        // ... existing streaming logic with translator ...
                    }
                    Err(e) => {
                        // Drop rl_tx so rl_rx gets Err -> unwrap_or_default()
                        drop(rl_tx);
                        send_stream_error(&tx, &metrics, e).await;
                    }
                }
            });
        }
        BackendClient::Gemini(client) => {
            let client = client.clone();
            let mapped_model = state.model_mapping.map_model(&body.model);
            let gemini_req = mapping::gemini_message_map::anthropic_to_gemini_request(&body);
            let original_model = body.model.clone();

            tokio::spawn(async move {
                match client.generate_content_stream(&gemini_req, &mapped_model).await {
                    Ok((response, rate_limits)) => {
                        // Gemini rate_limits will be default/empty
                        rl_tx.send(rate_limits).ok();
                        // ... existing streaming logic with gemini translator ...
                    }
                    Err(e) => {
                        drop(rl_tx);
                        send_stream_error(&tx, &metrics, e).await;
                    }
                }
            });
        }
    }

    let rate_limits = rl_rx.await.unwrap_or_default();
    (rate_limits, Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()))
}
```

Race condition analysis: `rl_rx.await` blocks until the spawned task sends rate limits (immediately after backend connection succeeds) or drops `rl_tx` (on connection failure). On success, rate limits arrive before any SSE data is sent on `tx`. On failure, `rl_rx` returns `Err`, handled by `unwrap_or_default()`. The error event is sent on `tx` which the client receives via the `rx` stream. No deadlock: `rl_tx` is always either sent or dropped before the spawned task progresses to streaming.

- [ ] **Step 9: Run all tests**

Run: `cargo test -p anyllm_proxy 2>&1`
Expected: All pass. The integration tests don't hit a real backend, so rate limit headers will be absent (default). The unit tests verify the mapping logic.

- [ ] **Step 10: Commit**

```bash
git add crates/proxy/src/backend/mod.rs crates/proxy/src/backend/openai_client.rs \
       crates/proxy/src/backend/gemini_client.rs crates/proxy/src/server/routes.rs
git commit -m "feat: passthrough backend rate limit headers as Anthropic equivalents (Phase 22)"
```

---

### Task 3: Error and Edge Case Fixtures

**Files:**
- Create: `fixtures/openai/error_401.json`
- Create: `fixtures/openai/error_429.json`
- Create: `fixtures/openai/error_500.json`
- Create: `fixtures/gemini/error_400.json`
- Create: `fixtures/gemini/error_429.json`
- Create: `fixtures/anthropic/error_invalid_request.json`
- Create: `fixtures/anthropic/error_rate_limit.json`
- Create: `fixtures/anthropic/messages_oversized_request.json`
- Create: `fixtures/openai/chat_completion_malformed.json`
- Modify: `crates/translator/src/mapping/errors_map.rs` (add fixture tests)
- Create: `crates/proxy/tests/error_fixtures.rs`

- [ ] **Step 1: Create OpenAI error fixtures**

`fixtures/openai/error_401.json`:
```json
{
    "error": {
        "message": "Incorrect API key provided: sk-...1234. You can find your API key at https://platform.openai.com/account/api-keys.",
        "type": "invalid_request_error",
        "param": null,
        "code": "invalid_api_key"
    }
}
```

`fixtures/openai/error_429.json`:
```json
{
    "error": {
        "message": "Rate limit reached for gpt-4o in organization org-xxx on tokens per min (TPM): Limit 30000, Used 28000, Requested 5000.",
        "type": "tokens",
        "param": null,
        "code": "rate_limit_exceeded"
    }
}
```

`fixtures/openai/error_500.json`:
```json
{
    "error": {
        "message": "The server had an error while processing your request. Sorry about that!",
        "type": "server_error",
        "param": null,
        "code": null
    }
}
```

`fixtures/openai/chat_completion_malformed.json` (missing required fields):
```json
{
    "id": "chatcmpl-abc123",
    "object": "chat.completion"
}
```

- [ ] **Step 2: Create Gemini error fixtures**

`fixtures/gemini/error_400.json`:
```json
{
    "error": {
        "code": 400,
        "message": "Invalid value at 'contents[0].parts[0]': Must have exactly one field set",
        "status": "INVALID_ARGUMENT"
    }
}
```

`fixtures/gemini/error_429.json`:
```json
{
    "error": {
        "code": 429,
        "message": "Resource has been exhausted (e.g. check quota).",
        "status": "RESOURCE_EXHAUSTED"
    }
}
```

- [ ] **Step 3: Create Anthropic error fixtures**

`fixtures/anthropic/error_invalid_request.json`:
```json
{
    "type": "error",
    "error": {
        "type": "invalid_request_error",
        "message": "max_tokens: Field required"
    }
}
```

`fixtures/anthropic/error_rate_limit.json`:
```json
{
    "type": "error",
    "error": {
        "type": "rate_limit_error",
        "message": "Number of request tokens has exceeded your per-minute rate limit"
    },
    "request_id": "req_01XYZ"
}
```

`fixtures/anthropic/messages_oversized_request.json` (a request with very long content):
```json
{
    "model": "claude-sonnet-4-6",
    "max_tokens": 1024,
    "messages": [
        {
            "role": "user",
            "content": "PLACEHOLDER_OVERSIZED"
        }
    ]
}
```

- [ ] **Step 4: Write fixture deserialization tests in errors_map.rs**

Add to `crates/translator/src/mapping/errors_map.rs` tests:

Note: `errors_map.rs` is at `crates/translator/src/mapping/errors_map.rs` (4 dirs deep from workspace root), so fixture paths need `../../../../fixtures/`.

```rust
#[test]
fn fixture_openai_error_401_deserializes() {
    let json = include_str!("../../../../fixtures/openai/error_401.json");
    let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.code.as_deref(), Some("invalid_api_key"));
}

#[test]
fn fixture_openai_error_429_deserializes() {
    let json = include_str!("../../../../fixtures/openai/error_429.json");
    let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert!(err.error.message.contains("Rate limit"));
}

#[test]
fn fixture_openai_error_500_deserializes() {
    let json = include_str!("../../../../fixtures/openai/error_500.json");
    let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.error_type, "server_error");
}

#[test]
fn fixture_gemini_error_400_deserializes() {
    let json = include_str!("../../../../fixtures/gemini/error_400.json");
    let err: gemini::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.code, 400);
    assert_eq!(err.error.status, "INVALID_ARGUMENT");
}

#[test]
fn fixture_gemini_error_429_deserializes() {
    let json = include_str!("../../../../fixtures/gemini/error_429.json");
    let err: gemini::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.code, 429);
}

#[test]
fn fixture_anthropic_error_invalid_request_deserializes() {
    let json = include_str!("../../../../fixtures/anthropic/error_invalid_request.json");
    let err: anthropic::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.error_type, anthropic::ErrorType::InvalidRequestError);
}

#[test]
fn fixture_anthropic_error_rate_limit_deserializes() {
    let json = include_str!("../../../../fixtures/anthropic/error_rate_limit.json");
    let err: anthropic::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.error_type, anthropic::ErrorType::RateLimitError);
    assert_eq!(err.request_id.as_deref(), Some("req_01XYZ"));
}
```

- [ ] **Step 5: Write error translation fixture tests**

Add to the same test module:

```rust
#[test]
fn fixture_openai_401_translates_to_anthropic_auth_error() {
    let json = include_str!("../../../../fixtures/openai/error_401.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 401, Some("req_test".into()));
    assert_eq!(anthropic_err.error.error_type, anthropic::ErrorType::AuthenticationError);
}

#[test]
fn fixture_openai_429_translates_to_anthropic_rate_limit() {
    let json = include_str!("../../../../fixtures/openai/error_429.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 429, None);
    assert_eq!(anthropic_err.error.error_type, anthropic::ErrorType::RateLimitError);
}

#[test]
fn fixture_openai_500_translates_to_anthropic_api_error() {
    let json = include_str!("../../../../fixtures/openai/error_500.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 500, None);
    assert_eq!(anthropic_err.error.error_type, anthropic::ErrorType::ApiError);
}

#[test]
fn fixture_gemini_400_translates_to_anthropic_invalid_request() {
    let json = include_str!("../../../../fixtures/gemini/error_400.json");
    let gemini_err: gemini::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = gemini_to_anthropic_error(&gemini_err, 400, None);
    assert_eq!(anthropic_err.error.error_type, anthropic::ErrorType::InvalidRequestError);
}

#[test]
fn fixture_gemini_429_translates_to_anthropic_rate_limit() {
    let json = include_str!("../../../../fixtures/gemini/error_429.json");
    let gemini_err: gemini::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = gemini_to_anthropic_error(&gemini_err, 429, None);
    assert_eq!(anthropic_err.error.error_type, anthropic::ErrorType::RateLimitError);
}
```

- [ ] **Step 6: Write integration test for malformed backend response**

Create `crates/proxy/tests/error_fixtures.rs`:

```rust
#[test]
fn malformed_openai_response_fails_deserialization() {
    let json = include_str!("../../../fixtures/openai/chat_completion_malformed.json");
    let result = serde_json::from_str::<anyllm_translate::openai::ChatCompletionResponse>(json);
    assert!(result.is_err(), "malformed response should fail deserialization");
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add fixtures/ crates/translator/src/mapping/errors_map.rs crates/proxy/tests/error_fixtures.rs
git commit -m "test: add error/edge case fixtures for OpenAI, Gemini, Anthropic (Phase 22)"
```

---

### Task 4: Request/Response Logging Toggle

**Files:**
- Modify: `crates/proxy/src/config.rs`
- Modify: `crates/proxy/src/server/routes.rs`
- Modify: `crates/proxy/src/server/middleware.rs`
- Create: `crates/proxy/tests/body_logging.rs`

Design: Add `LOG_BODIES=true|false` env var (default false). When enabled, a middleware logs the request body (redacted: API keys replaced with `[REDACTED]`) at `debug` level, and the response body at `debug` level. Only applies to `/v1/messages` route. Keep it simple: log the raw JSON, redact known secret patterns.

- [ ] **Step 1: Add config field**

In `crates/proxy/src/config.rs`, add to the `Config` struct:

```rust
/// Enable request/response body logging at debug level.
/// Bodies are redacted: API keys and bearer tokens are replaced with [REDACTED].
pub log_bodies: bool,
```

In `Config::from_env()`:
```rust
log_bodies: std::env::var("LOG_BODIES")
    .map(|v| v == "true" || v == "1")
    .unwrap_or(false),
```

- [ ] **Step 2: Verify the field compiles into existing test helpers**

No separate test needed for the bool field. Verification comes from updating `test_config()` in integration tests (Step 8) and the integration test in Step 6.

- [ ] **Step 3: Add log_bodies to AppState**

In `crates/proxy/src/server/routes.rs`, add `log_bodies: bool` to `AppState` and wire it from config:

```rust
pub struct AppState {
    pub backend: BackendClient,
    pub metrics: Metrics,
    pub model_mapping: ModelMapping,
    pub log_bodies: bool,
}
```

In `app()`:
```rust
let state = AppState {
    backend: BackendClient::new(&config),
    metrics: Metrics::new(),
    model_mapping: config.model_mapping.clone(),
    log_bodies: config.log_bodies,
};
```

- [ ] **Step 4: (No separate redaction utility needed)**

Body content is LLM conversation text, not secrets. API keys live in headers (which we already don't log). When `log_bodies` is true, log the serialized request/response body at `tracing::debug!` level. The user opts in via both `LOG_BODIES=true` and `RUST_LOG=debug`. No redaction of body content.

- [ ] **Step 5: Add body logging to the messages handler**

In `crates/proxy/src/server/routes.rs`, at the start of `messages()`:

```rust
async fn messages(
    State(state): State<AppState>,
    Json(body): Json<anthropic::MessageCreateRequest>,
) -> Response {
    state.metrics.record_request();

    if state.log_bodies {
        tracing::debug!(
            model = %body.model,
            stream = ?body.stream,
            message_count = body.messages.len(),
            body = %serde_json::to_string(&body).unwrap_or_else(|_| "[serialization failed]".into()),
            "request body"
        );
    }

    // ... existing handler logic ...
    // After getting the response, before returning:
    // if state.log_bodies {
    //     tracing::debug!(body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()), "response body");
    // }
}
```

Add response logging at each success path in both non-streaming branches. For streaming, log that streaming was initiated (body logging of SSE frames would be too verbose).

- [ ] **Step 6: Write integration test**

Create `crates/proxy/tests/body_logging.rs`:

```rust
use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;

fn test_config_with_logging() -> Config {
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
        log_bodies: true,
    }
}

#[tokio::test]
async fn server_starts_with_body_logging_enabled() {
    // Just verify the config wires through without panic
    let app = routes::app(test_config_with_logging());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test 2>&1`
Expected: All pass.

- [ ] **Step 8: Update test helpers**

All existing `test_config()` functions in integration tests need the new `log_bodies: false` field. Update:
- `crates/proxy/tests/compatibility.rs`: `test_config()`
- `crates/proxy/tests/health.rs`: uses `Config::from_env()`, no change needed
- `crates/proxy/tests/shutdown.rs`: `test_config()`
- Any `#[cfg(test)]` in proxy crate source files that construct `Config`

- [ ] **Step 9: Run all tests and clippy**

Run: `cargo test && cargo clippy -- -D warnings 2>&1`
Expected: All clean.

- [ ] **Step 10: Commit**

```bash
git add crates/proxy/src/config.rs crates/proxy/src/server/routes.rs \
       crates/proxy/src/server/middleware.rs crates/proxy/tests/body_logging.rs \
       crates/proxy/tests/compatibility.rs crates/proxy/tests/shutdown.rs
git commit -m "feat: add LOG_BODIES toggle for opt-in request/response debug logging (Phase 22)"
```

---

### Task 5: Update TASKS.md and Final Verification

**Files:**
- Modify: `TASKS.md`

- [ ] **Step 1: Mark Phase 22 items as complete**

Update TASKS.md Phase 22 section:

```markdown
## Phase 22: Hardening and Operability

- [x] Graceful shutdown with in-flight request draining (SIGINT + SIGTERM)
- [x] Rate limit header passthrough (OpenAI rate limit headers to Anthropic equivalents)
- [x] Error/edge case fixtures (4xx/5xx responses, malformed JSON)
- [x] Request/response logging toggle (opt-in body logging for debugging, redacted by default)
- [ ] OpenAI Responses API backend: wire up `ResponsesRequest`/`ResponsesResponse` types with runtime backend selection
- [ ] Live API integration tests (requires OPENAI_API_KEY, currently golden fixtures only)
- [ ] Publish `anyllm_translate` crate to crates.io
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt --check 2>&1`
Expected: All clean.

- [ ] **Step 3: Commit**

```bash
git add TASKS.md
git commit -m "docs: update TASKS.md with completed Phase 22 items"
```
