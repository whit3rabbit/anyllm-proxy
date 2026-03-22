use crate::backend::{BackendClient, BackendError, RateLimitHeaders};
use crate::config::{Config, ModelMapping};
use crate::metrics::Metrics;
use anthropic_openai_translate::{anthropic, gemini, mapping, openai};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use std::sync::LazyLock;
use tiktoken_rs::CoreBPE;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// GPT-4o family tokenizer, initialized once. Used for approximate token counting.
static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::o200k_base().expect("failed to load o200k_base tokenizer"));

/// Shared application state for all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub backend: BackendClient,
    pub metrics: Metrics,
    pub model_mapping: ModelMapping,
    pub log_bodies: bool,
}

/// Build the axum router emulating Anthropic's POST /v1/messages endpoint.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn app(config: Config) -> Router {
    let state = AppState {
        backend: BackendClient::new(&config),
        metrics: Metrics::new(),
        model_mapping: config.model_mapping.clone(),
        log_bodies: config.log_bodies,
    };

    // Auth-protected API routes with concurrency limit.
    // ConcurrencyLimit prevents self-DOS under upstream 429 incidents.
    let api_routes = Router::new()
        .route("/v1/messages", post(messages))
        .route("/v1/models", get(models))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .route("/v1/messages/batches", post(batches))
        .layer(axum::middleware::from_fn(super::middleware::validate_auth))
        .layer(axum::middleware::from_fn(
            super::middleware::log_anthropic_headers,
        ))
        .layer(DefaultBodyLimit::max(super::middleware::MAX_BODY_SIZE))
        .layer(tower::limit::ConcurrencyLimitLayer::new(
            super::middleware::MAX_CONCURRENT_REQUESTS,
        ));

    // Health and metrics are public, bypass auth and concurrency limits.
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .merge(api_routes)
        .layer(axum::middleware::from_fn(super::middleware::add_request_id))
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

async fn count_tokens(Json(body): Json<anthropic::MessageCreateRequest>) -> impl IntoResponse {
    let text = extract_text_for_counting(&body);
    let token_count = TOKENIZER.encode_with_special_tokens(&text).len();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "input_tokens": token_count })),
    )
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

async fn metrics_endpoint(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snap = state.metrics.snapshot();
    Json(serde_json::json!(snap))
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
            message: error.to_string(),
        },
    };
    if let Ok(sse) = super::sse::stream_event_to_sse(&err_event) {
        let _ = tx.send(Ok(sse)).await;
    }
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
    let mut buffer = String::new();
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
        match std::str::from_utf8(&bytes) {
            Ok(s) => buffer.push_str(s),
            Err(e) => {
                tracing::warn!("non-UTF-8 chunk from backend: {e}");
                buffer.push_str(&String::from_utf8_lossy(&bytes));
            }
        }

        while let Some(pos) = buffer.find("\n\n") {
            frame_events.clear();
            for line in buffer[..pos].lines() {
                let line = line.trim();
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Some(mut events) = on_data(json_str) {
                        frame_events.append(&mut events);
                    }
                }
            }
            buffer.drain(..pos + 2);

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
async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
) -> (
    RateLimitHeaders,
    Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>,
) {
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
                        if completed {
                            metrics.record_success();
                        }
                    }
                    Err(e) => {
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
                match client
                    .generate_content_stream(&gemini_req, &mapped_model)
                    .await
                {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(rate_limits).ok();
                        let mut translator =
                            mapping::gemini_streaming_map::GeminiStreamingTranslator::new(
                                original_model,
                            );

                        let completed = read_sse_frames(response, &tx, &metrics, |json_str| {
                            match serde_json::from_str::<gemini::GenerateContentResponse>(
                                json_str,
                            ) {
                                Ok(chunk) => Some(translator.process_chunk(&chunk)),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to parse Gemini streaming chunk: {e}"
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

    // Transport or deserialization error
    tracing::error!("backend client error: {error}");
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        format!("Upstream error: {error}"),
        None,
    );
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
}

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

    if body.stream == Some(true) {
        if state.log_bodies {
            tracing::debug!(model = %body.model, "streaming request initiated");
        }
        let (rate_limits, sse) = messages_stream(state, body).await;
        let mut response = sse.into_response();
        rate_limits.inject_anthropic_headers(response.headers_mut());
        return response;
    }

    match &state.backend {
        BackendClient::OpenAI(client) | BackendClient::Vertex(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            openai_req.model = state.model_mapping.map_model(&openai_req.model);
            let original_model = body.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );
                    if state.log_bodies {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_headers(response.headers_mut());
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Gemini(client) => {
            let mapped_model = state.model_mapping.map_model(&body.model);
            let gemini_req = mapping::gemini_message_map::anthropic_to_gemini_request(&body);
            let original_model = body.model.clone();

            match client.generate_content(&gemini_req, &mapped_model).await {
                Ok((gemini_resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp = mapping::gemini_message_map::gemini_to_anthropic_response(
                        &gemini_resp,
                        &original_model,
                    );
                    if state.log_bodies {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_headers(response.headers_mut());
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
    }
}
