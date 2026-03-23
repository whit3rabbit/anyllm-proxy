use crate::admin::state::{AdminEvent, RequestLogEntry, RuntimeConfig, SharedState};
use crate::backend::{BackendClient, BackendError, RateLimitHeaders};
use crate::config::{BackendKind, Config, MultiConfig};
use crate::metrics::Metrics;
use anthropic_openai_translate::{anthropic, gemini, mapping, openai};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use bytes::BytesMut;
use futures::stream::Stream;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use tiktoken_rs::CoreBPE;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// GPT-4o family tokenizer, initialized once. Used for approximate token counting.
static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::o200k_base().expect("failed to load o200k_base tokenizer"));

/// Per-backend state shared across request handlers for one backend.
#[derive(Clone)]
pub struct AppState {
    pub backend: BackendClient,
    pub metrics: Metrics,
    /// Runtime config (model mappings, log_bodies) read on every request.
    /// Shared with admin server so config changes take effect immediately.
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    /// Shared admin state for request logging and live updates. None in tests.
    pub shared: Option<SharedState>,
    /// Backend name for logging purposes.
    pub backend_name: String,
}

impl AppState {
    /// Map a model name through the current runtime config for this backend.
    fn map_model(&self, model: &str) -> String {
        let config = self.runtime_config.read().unwrap();
        if let Some(mapping) = config.model_mappings.get(&self.backend_name) {
            mapping.map_model(model)
        } else {
            model.to_string()
        }
    }

    /// Whether request/response body logging is enabled.
    fn log_bodies(&self) -> bool {
        self.runtime_config.read().unwrap().log_bodies
    }
}

/// Global state for the multi-backend metrics endpoint.
#[derive(Clone)]
struct GlobalState {
    backend_metrics: Arc<HashMap<String, Metrics>>,
}

/// Build the axum router from a legacy single-backend Config.
pub fn app(config: Config) -> Router {
    let multi = MultiConfig::from_single_config(&config);
    app_multi(multi)
}

/// Build the axum router from multi-backend configuration.
/// Creates nested sub-routers for each configured backend.
pub fn app_multi(config: MultiConfig) -> Router {
    app_multi_with_shared(config, None)
}

/// Build the axum router with optional shared admin state.
pub fn app_multi_with_shared(config: MultiConfig, shared: Option<SharedState>) -> Router {
    let mut backend_metrics: HashMap<String, Metrics> = HashMap::new();
    let mut router = Router::new();

    // When no shared state (tests), build a standalone runtime config from the multi config.
    let runtime_config: Arc<RwLock<RuntimeConfig>> = if let Some(ref s) = shared {
        s.runtime_config.clone()
    } else {
        let mut model_mappings = indexmap::IndexMap::new();
        for (name, bc) in &config.backends {
            model_mappings.insert(name.clone(), bc.model_mapping.clone());
        }
        Arc::new(RwLock::new(RuntimeConfig {
            model_mappings,
            log_level: "info".to_string(),
            log_bodies: config.log_bodies,
        }))
    };

    // Build per-backend sub-routers
    for (name, bc) in &config.backends {
        let metrics = Metrics::new();
        backend_metrics.insert(name.clone(), metrics.clone());

        let state = AppState {
            backend: BackendClient::from_backend_config(bc),
            metrics,
            runtime_config: runtime_config.clone(),
            shared: shared.clone(),
            backend_name: name.clone(),
        };

        let is_anthropic = bc.kind == BackendKind::Anthropic;
        let sub = backend_router(state, is_anthropic);

        // Nest under /{name}/
        router = router.nest(&format!("/{name}"), sub);
    }

    // Default backend: also serve at un-prefixed /v1/messages for backward compat
    if let Some(bc) = config.backends.get(&config.default_backend) {
        let default_metrics = backend_metrics
            .get(&config.default_backend)
            .cloned()
            .unwrap_or_else(Metrics::new);

        let default_state = AppState {
            backend: BackendClient::from_backend_config(bc),
            metrics: default_metrics,
            runtime_config: runtime_config.clone(),
            shared: shared.clone(),
            backend_name: config.default_backend.clone(),
        };

        let is_anthropic = bc.kind == BackendKind::Anthropic;
        let default_sub = backend_router(default_state, is_anthropic);
        router = router.merge(default_sub);
    }

    let global_state = GlobalState {
        backend_metrics: Arc::new(backend_metrics),
    };

    // Health and metrics are public, bypass auth and concurrency limits.
    Router::new()
        .route("/health", get(health))
        .route(
            "/metrics",
            get(|State(gs): State<GlobalState>| async move {
                let mut backends = serde_json::Map::new();
                let mut total_requests: u64 = 0;
                let mut total_success: u64 = 0;
                let mut total_error: u64 = 0;
                for (name, m) in gs.backend_metrics.iter() {
                    let snap = m.snapshot();
                    total_requests += snap.requests_total;
                    total_success += snap.requests_success;
                    total_error += snap.requests_error;
                    backends.insert(
                        name.clone(),
                        serde_json::to_value(&snap).unwrap_or_default(),
                    );
                }
                Json(serde_json::json!({
                    "backends": backends,
                    "total": {
                        "requests_total": total_requests,
                        "requests_success": total_success,
                        "requests_error": total_error,
                    }
                }))
            }),
        )
        .merge(router)
        .layer(axum::middleware::from_fn(super::middleware::add_request_id))
        .with_state(global_state)
}

/// Build the sub-router for a single backend.
/// If `is_anthropic` is true, uses the passthrough handler instead of the translation handler.
fn backend_router(state: AppState, is_anthropic: bool) -> Router<GlobalState> {
    let api_routes = if is_anthropic {
        Router::new()
            .route("/v1/messages", post(anthropic_passthrough))
            .route("/v1/models", get(models))
    } else {
        Router::new()
            .route("/v1/messages", post(messages))
            .route("/v1/models", get(models))
            .route("/v1/messages/count_tokens", post(count_tokens))
            .route("/v1/messages/batches", post(batches))
    };

    api_routes
        .layer(axum::middleware::from_fn(super::middleware::validate_auth))
        .layer(axum::middleware::from_fn(
            super::middleware::log_anthropic_headers,
        ))
        .layer(DefaultBodyLimit::max(super::middleware::MAX_BODY_SIZE))
        .layer(tower::limit::ConcurrencyLimitLayer::new(
            super::middleware::MAX_CONCURRENT_REQUESTS,
        ))
        .with_state(state)
}

static MODELS_RESPONSE: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::json!({
        "data": [
            {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
            {"id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
            {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
        ],
        "has_more": false,
        "first_id": "claude-opus-4-6",
        "last_id": "claude-haiku-4-5-20251001",
    })
});

async fn models(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(MODELS_RESPONSE.clone())
}

async fn count_tokens(Json(body): Json<anthropic::MessageCreateRequest>) -> Response {
    // Tokenization is CPU-bound; offload to the blocking threadpool.
    match tokio::task::spawn_blocking(move || {
        let text = extract_text_for_counting(&body);
        TOKENIZER.encode_with_special_tokens(&text).len()
    })
    .await
    {
        Ok(token_count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "input_tokens": token_count })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "token counting failed" })),
        )
            .into_response(),
    }
}

/// Flatten an Anthropic request into a single string for token counting.
/// Covers system prompt, messages (text, tool_use, tool_result, thinking), and tool definitions.
fn extract_text_for_counting(req: &anthropic::MessageCreateRequest) -> String {
    let mut buf = String::new();

    if let Some(system) = &req.system {
        match system {
            anthropic::System::Text(t) => append(&mut buf, t),
            anthropic::System::Blocks(blocks) => {
                for b in blocks {
                    append(&mut buf, &b.text);
                }
            }
        }
    }

    for msg in &req.messages {
        append_content(&msg.content, &mut buf);
    }

    if let Some(tools) = &req.tools {
        for tool in tools {
            append(&mut buf, &tool.name);
            if let Some(desc) = &tool.description {
                append(&mut buf, desc);
            }
            if let Ok(schema) = serde_json::to_string(&tool.input_schema) {
                append(&mut buf, &schema);
            }
        }
    }

    buf
}

/// Append a text segment to the buffer, separated by newline from prior content.
fn append(buf: &mut String, text: &str) {
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(text);
}

fn append_content(content: &anthropic::Content, buf: &mut String) {
    match content {
        anthropic::Content::Text(t) => append(buf, t),
        anthropic::Content::Blocks(blocks) => {
            for block in blocks {
                match block {
                    anthropic::ContentBlock::Text { text } => append(buf, text),
                    anthropic::ContentBlock::ToolUse { name, input, .. } => {
                        append(buf, name);
                        if let Ok(s) = serde_json::to_string(input) {
                            append(buf, &s);
                        }
                    }
                    anthropic::ContentBlock::ToolResult {
                        content: Some(c), ..
                    } => match c {
                        anthropic::messages::ToolResultContent::Text(t) => append(buf, t),
                        anthropic::messages::ToolResultContent::Blocks(inner) => {
                            for b in inner {
                                if let anthropic::ContentBlock::Text { text } = b {
                                    append(buf, text);
                                }
                            }
                        }
                    },
                    anthropic::ContentBlock::Thinking { thinking, .. } => {
                        append(buf, thinking);
                    }
                    // Image and Document blocks are not text-tokenizable
                    _ => {}
                }
            }
        }
    }
}

async fn batches() -> impl IntoResponse {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::InvalidRequestError,
        "Batch processing is not supported by this proxy.".to_string(),
        None,
    );
    (StatusCode::BAD_REQUEST, Json(err))
}

async fn health() -> impl IntoResponse {
    ([("content-type", "application/json")], r#"{"status":"ok"}"#)
}

/// Anthropic passthrough: forward raw request bytes to the real Anthropic API.
/// No translation: the proxy receives Anthropic format and returns Anthropic format.
async fn anthropic_passthrough(State(state): State<AppState>, body: Bytes) -> Response {
    state.metrics.record_request();

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Check if the request is a streaming request by peeking at the JSON.
    // We parse minimally to avoid rejecting valid requests.
    let is_stream = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("stream")?.as_bool())
        .unwrap_or(false);

    if is_stream {
        match client.forward_stream(body).await {
            Ok((response, rate_limits)) => {
                state.metrics.record_success();
                // Pipe the raw SSE stream through to the client
                let stream = response.bytes_stream();
                let mut resp = axum::body::Body::from_stream(stream).into_response();
                resp.headers_mut()
                    .insert("content-type", "text/event-stream".parse().unwrap());
                resp.headers_mut()
                    .insert("cache-control", "no-cache".parse().unwrap());
                rate_limits.inject_anthropic_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
        }
    } else {
        match client.forward(body).await {
            Ok((resp_body, rate_limits)) => {
                state.metrics.record_success();
                let mut resp = (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    resp_body,
                )
                    .into_response();
                rate_limits.inject_anthropic_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
        }
    }
}

/// Convert an AnthropicClientError into a Response.
/// For API errors, return the upstream error body directly (it's already Anthropic format).
fn passthrough_error_to_response(
    error: crate::backend::anthropic_client::AnthropicClientError,
) -> Response {
    use crate::backend::anthropic_client::AnthropicClientError;
    match error {
        AnthropicClientError::ApiError { status, body } => {
            let http_status =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (http_status, [("content-type", "application/json")], body).into_response()
        }
        AnthropicClientError::Transport(msg) => {
            tracing::error!("Anthropic passthrough transport error: {msg}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
    }
}

/// Send translated stream events over the SSE channel. Returns false if client disconnected.
async fn send_events(
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    events: &[anthropic::StreamEvent],
) -> bool {
    for ev in events {
        match super::sse::stream_event_to_sse(ev) {
            Ok(sse) => {
                if tx.send(Ok(sse)).await.is_err() {
                    return false;
                }
            }
            Err(e) => {
                tracing::warn!("failed to serialize stream event: {e}");
            }
        }
    }
    true
}

/// Send an SSE error event over the channel.
/// Logs the detailed error server-side and sends a generic message to the client.
async fn send_stream_error(
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    metrics: &Metrics,
    error: impl std::fmt::Display,
) {
    tracing::error!("streaming request failed: {error}");
    metrics.record_error();
    let err_event = anthropic::StreamEvent::Error {
        error: anthropic::streaming::StreamError {
            error_type: "api_error".to_string(),
            message: "An internal error occurred while communicating with the upstream service."
                .to_string(),
        },
    };
    if let Ok(sse) = super::sse::stream_event_to_sse(&err_event) {
        let _ = tx.send(Ok(sse)).await;
    }
}

/// Maximum SSE buffer size (10 MB). Protects against unbounded memory growth
/// if the backend sends data without frame delimiters.
const MAX_SSE_BUFFER_SIZE: usize = 10 * 1024 * 1024;

/// Find the first SSE frame boundary (`\n\n` or `\r\n\r\n`) in a byte slice.
/// Returns `(position, delimiter_length)` so the caller can skip the full delimiter.
fn find_double_newline(buf: &[u8]) -> Option<(usize, usize)> {
    let len = buf.len();
    let mut i = 0;
    while i < len.saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if buf[i] == b'\r' && i + 3 < len && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

/// Read SSE bytes from a response, parse frames, and call `on_data` for each data line.
/// Returns true if stream completed normally, false if client disconnected.
async fn read_sse_frames<F>(
    response: reqwest::Response,
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    metrics: &Metrics,
    mut on_data: F,
) -> bool
where
    F: FnMut(&str) -> Option<Vec<anthropic::StreamEvent>>,
{
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    // Use a byte buffer to avoid corrupting multi-byte UTF-8 characters
    // split across TCP chunk boundaries. String::from_utf8_lossy would
    // permanently replace partial trailing bytes with U+FFFD.
    let mut buffer = BytesMut::new();
    // Reuse a single events buffer across all frames to avoid per-frame allocation
    let mut frame_events: Vec<anthropic::StreamEvent> = Vec::new();

    while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("stream read error: {e}");
                metrics.record_error();
                return false;
            }
        };
        buffer.extend_from_slice(&bytes);

        // Guard against unbounded buffer growth from a misbehaving backend.
        if buffer.len() > MAX_SSE_BUFFER_SIZE {
            tracing::error!(
                buffer_len = buffer.len(),
                "SSE buffer exceeded maximum size, aborting stream"
            );
            metrics.record_error();
            return false;
        }

        while let Some((pos, delim_len)) = find_double_newline(&buffer) {
            frame_events.clear();
            // Convert the complete frame bytes to UTF-8. A frame ending at
            // a double-newline boundary should always be valid UTF-8; if not,
            // skip the malformed frame rather than injecting replacement chars.
            match std::str::from_utf8(&buffer[..pos]) {
                Ok(frame_str) => {
                    for line in frame_str.lines() {
                        let line = line.trim();
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            if let Some(mut events) = on_data(json_str) {
                                frame_events.append(&mut events);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("skipping non-UTF-8 SSE frame: {e}");
                }
            }
            let _ = buffer.split_to(pos + delim_len);

            if !send_events(tx, &frame_events).await {
                tracing::debug!("client disconnected during stream");
                return false;
            }
        }
    }

    true
}

/// Build an SSE response that streams Anthropic events translated from backend chunks.
/// Returns rate limit headers alongside the SSE stream so the caller can inject them.
/// Logging is deferred: each spawned task logs after the stream completes with actual
/// latency, status, and token counts.
async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
    ctx: RequestCtx,
    mapped_model: String,
) -> (
    RateLimitHeaders,
    Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>,
) {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);
    let (rl_tx, rl_rx) = tokio::sync::oneshot::channel::<RateLimitHeaders>();

    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();

    match &state.backend {
        BackendClient::OpenAI(client) | BackendClient::Vertex(client) => {
            let client = client.clone();
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            openai_req.model = state.map_model(&openai_req.model);
            let model = body.model.clone();

            tokio::spawn(async move {
                match client.chat_completion_stream(&openai_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(rate_limits).ok();
                        let mut translator =
                            mapping::streaming_map::StreamingTranslator::new(model);
                        let mut done = false;

                        let completed = read_sse_frames(response, &tx, &metrics, |json_str| {
                            if json_str == "[DONE]" {
                                done = true;
                                let events = translator.finish();
                                return Some(events);
                            }
                            match serde_json::from_str::<openai::ChatCompletionChunk>(json_str) {
                                Ok(chunk) => Some(translator.process_chunk(&chunk)),
                                Err(e) => {
                                    tracing::debug!("failed to parse OpenAI streaming chunk: {e}");
                                    None
                                }
                            }
                        })
                        .await;

                        if completed && !done {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                        }
                        let usage = translator.usage();
                        let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                        if completed {
                            metrics.record_success();
                        }
                        let (status, err) = if completed {
                            (200, None)
                        } else {
                            (502, Some("stream interrupted".into()))
                        };
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, tokens, true, err,
                        ));
                    }
                    Err(e) => {
                        let status = e.status_code();
                        let err_msg = e.to_string();
                        drop(rl_tx);
                        send_stream_error(&tx, &metrics, e).await;
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, None, true, Some(err_msg),
                        ));
                    }
                }
            });
        }
        BackendClient::OpenAIResponses(client) => {
            let client = client.clone();
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = state.map_model(&responses_req.model);
            responses_req.stream = Some(true);
            let model = body.model.clone();

            tokio::spawn(async move {
                match client.responses_stream(&responses_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(rate_limits).ok();
                        let mut translator =
                            mapping::responses_streaming_map::ResponsesStreamingTranslator::new(
                                model,
                            );

                        let completed = read_sse_frames(response, &tx, &metrics, |json_str| {
                            match serde_json::from_str::<
                                mapping::responses_streaming_map::ResponsesStreamEvent,
                            >(json_str)
                            {
                                Ok(event) => Some(translator.process_event(&event)),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to parse Responses API streaming event: {e}"
                                    );
                                    None
                                }
                            }
                        })
                        .await;

                        if completed {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                            metrics.record_success();
                        }
                        // Responses API translator does not expose usage yet.
                        let (status, err) = if completed {
                            (200, None)
                        } else {
                            (502, Some("stream interrupted".into()))
                        };
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, None, true, err,
                        ));
                    }
                    Err(e) => {
                        let status = e.status_code();
                        let err_msg = e.to_string();
                        drop(rl_tx);
                        send_stream_error(&tx, &metrics, e).await;
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, None, true, Some(err_msg),
                        ));
                    }
                }
            });
        }
        BackendClient::Gemini(client) => {
            let client = client.clone();
            let gemini_mapped = state.map_model(&body.model);
            let gemini_req = mapping::gemini_message_map::anthropic_to_gemini_request(&body);
            let original_model = body.model.clone();

            tokio::spawn(async move {
                match client
                    .generate_content_stream(&gemini_req, &gemini_mapped)
                    .await
                {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(rate_limits).ok();
                        let mut translator =
                            mapping::gemini_streaming_map::GeminiStreamingTranslator::new(
                                original_model,
                            );

                        let completed = read_sse_frames(response, &tx, &metrics, |json_str| {
                            match serde_json::from_str::<gemini::GenerateContentResponse>(json_str)
                            {
                                Ok(chunk) => Some(translator.process_chunk(&chunk)),
                                Err(e) => {
                                    tracing::debug!("failed to parse Gemini streaming chunk: {e}");
                                    None
                                }
                            }
                        })
                        .await;

                        if completed {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                            metrics.record_success();
                        }
                        let (status, err) = if completed {
                            (200, None)
                        } else {
                            (502, Some("stream interrupted".into()))
                        };
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, None, true, err,
                        ));
                    }
                    Err(e) => {
                        let status = e.status_code();
                        let err_msg = e.to_string();
                        drop(rl_tx);
                        send_stream_error(&tx, &metrics, e).await;
                        log_request(&log_shared, ctx.log_entry(
                            &log_backend_name, Some(mapped_model), status, None, true, Some(err_msg),
                        ));
                    }
                }
            });
        }
        BackendClient::Anthropic(_) => {
            drop(rl_tx);
            let _ = tx
                .send(Ok(Event::default().data(
                    r#"{"error":"anthropic passthrough does not use this handler"}"#,
                )))
                .await;
        }
    }

    let rate_limits = rl_rx.await.unwrap_or_default();
    (
        rate_limits,
        Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
    )
}

/// Convert a BackendError into an Anthropic error Response.
fn backend_error_to_response(error: BackendError) -> Response {
    if let Some((message, status)) = error.api_error_details() {
        let anthropic_err = mapping::errors_map::status_to_anthropic_error(status, message, None);
        let http_status = StatusCode::from_u16(
            mapping::errors_map::anthropic_error_type_to_status(&anthropic_err.error.error_type),
        )
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (http_status, Json(anthropic_err)).into_response();
    }

    // Transport or deserialization error -- log details server-side only,
    // return a generic message to avoid leaking infrastructure details.
    tracing::error!("backend client error: {error}");
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        "An internal error occurred while communicating with the upstream service.".to_string(),
        None,
    );
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
}

async fn messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<anthropic::MessageCreateRequest>,
) -> Response {
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: body.model.clone(),
    };
    state.metrics.record_request();

    if state.log_bodies() {
        tracing::debug!(
            model = %body.model,
            stream = ?body.stream,
            message_count = body.messages.len(),
            body = %serde_json::to_string(&body).unwrap_or_else(|_| "[serialization failed]".into()),
            "request body"
        );
    }

    if body.stream == Some(true) {
        if state.log_bodies() {
            tracing::debug!(model = %body.model, "streaming request initiated");
        }
        let mapped_model = state.map_model(&body.model);
        // Logging deferred until stream completes (inside messages_stream tasks).
        let (rate_limits, sse) = messages_stream(state, body, ctx, mapped_model).await;
        let mut response = sse.into_response();
        rate_limits.inject_anthropic_headers(response.headers_mut());
        return response;
    }

    match &state.backend {
        BackendClient::OpenAI(client) | BackendClient::Vertex(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            openai_req.model = state.map_model(&openai_req.model);
            let mapped_model = openai_req.model.clone();
            let original_model = body.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );
                    if state.log_bodies() {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                        ),
                    );
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_headers(response.headers_mut());
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                        ),
                    );
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = state.map_model(&responses_req.model);
            let mapped_model = responses_req.model.clone();
            let original_model = body.model.clone();

            match client.responses(&responses_req).await {
                Ok((resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp =
                        mapping::responses_message_map::responses_to_anthropic_response(
                            &resp,
                            &original_model,
                        );
                    if state.log_bodies() {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                        ),
                    );
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_headers(response.headers_mut());
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                        ),
                    );
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Gemini(client) => {
            let mapped_model = state.map_model(&body.model);
            let gemini_req = mapping::gemini_message_map::anthropic_to_gemini_request(&body);
            let original_model = body.model.clone();

            match client.generate_content(&gemini_req, &mapped_model).await {
                Ok((gemini_resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp = mapping::gemini_message_map::gemini_to_anthropic_response(
                        &gemini_resp,
                        &original_model,
                    );
                    if state.log_bodies() {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model.clone()),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                        ),
                    );
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_headers(response.headers_mut());
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                        ),
                    );
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Anthropic(_) => {
            // Anthropic passthrough is handled by a separate handler that works with raw bytes.
            // If we reach here, something is misconfigured.
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Anthropic passthrough does not use the translation handler".to_string(),
                None,
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}

/// Captures per-request context shared across success/error log paths.
struct RequestCtx {
    request_id: String,
    start: std::time::Instant,
    model_requested: String,
}

impl RequestCtx {
    /// Build a log entry, filling common fields from the context.
    fn log_entry(
        &self,
        backend_name: &str,
        model_mapped: Option<String>,
        status_code: u16,
        tokens: Option<(u64, u64)>,
        is_streaming: bool,
        error_message: Option<String>,
    ) -> RequestLogEntry {
        RequestLogEntry {
            request_id: self.request_id.clone(),
            timestamp: crate::admin::db::now_iso8601(),
            backend: backend_name.to_string(),
            model_requested: Some(self.model_requested.clone()),
            model_mapped,
            status_code,
            latency_ms: self.start.elapsed().as_millis() as u64,
            input_tokens: tokens.map(|(i, _)| i),
            output_tokens: tokens.map(|(_, o)| o),
            is_streaming,
            error_message,
        }
    }
}

/// Log a completed request to the admin write buffer and broadcast to WebSocket clients.
fn log_request(shared: &Option<SharedState>, entry: RequestLogEntry) {
    if let Some(ref shared) = shared {
        let _ = shared
            .events_tx
            .send(AdminEvent::RequestCompleted(entry.clone()));
        let _ = shared.log_tx.try_send(entry);
    }
}
