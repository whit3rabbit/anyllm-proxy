use crate::backend::openai_client::{OpenAIClient, OpenAIClientError};
use crate::config::{Config, ModelMapping};
use crate::metrics::Metrics;
use anthropic_openai_translate::{anthropic, mapping, openai};
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
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Shared application state for all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub openai_client: OpenAIClient,
    pub metrics: Metrics,
    pub model_mapping: ModelMapping,
}

/// Build the axum router emulating Anthropic's POST /v1/messages endpoint.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn app(config: Config) -> Router {
    let state = AppState {
        openai_client: OpenAIClient::new(&config),
        metrics: Metrics::new(),
        model_mapping: config.model_mapping.clone(),
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

async fn models(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "data": [
            {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
            {"id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
            {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "created_at": "2025-05-14T00:00:00Z", "type": "model"},
        ],
        "has_more": false,
        "first_id": "claude-opus-4-6",
        "last_id": "claude-haiku-4-5-20251001",
    }))
}

async fn count_tokens() -> impl IntoResponse {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::InvalidRequestError,
        "Token counting is not supported by this proxy.".to_string(),
        None,
    );
    (StatusCode::BAD_REQUEST, Json(err))
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
        if let Ok(sse) = super::sse::stream_event_to_sse(ev) {
            if tx.send(Ok(sse)).await.is_err() {
                return false;
            }
        }
    }
    true
}

/// Build an SSE response that streams Anthropic events translated from OpenAI chunks.
async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
    openai_req.model = state.model_mapping.map_model(&openai_req.model);

    let model = body.model.clone();

    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);

    let metrics = state.metrics.clone();
    tokio::spawn(async move {
        match state
            .openai_client
            .chat_completion_stream(&openai_req)
            .await
        {
            Ok(response) => {
                let mut translator = mapping::streaming_map::StreamingTranslator::new(model);
                let mut stream = response.bytes_stream();
                use futures::StreamExt;
                let mut buffer = String::new();

                while let Some(chunk_result) = stream.next().await {
                    let bytes = match chunk_result {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("stream read error: {e}");
                            metrics.record_error();
                            break;
                        }
                    };
                    match std::str::from_utf8(&bytes) {
                        Ok(s) => buffer.push_str(s),
                        Err(e) => {
                            tracing::warn!("non-UTF-8 chunk from OpenAI: {e}");
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                    }

                    // Process complete SSE frames delimited by double newline
                    while let Some(pos) = buffer.find("\n\n") {
                        let frame = &buffer[..pos];

                        for line in frame.lines() {
                            let line = line.trim();
                            if line == "data: [DONE]" {
                                let events = translator.finish();
                                send_events(&tx, &events).await;
                                metrics.record_success();
                                return;
                            }
                            if let Some(json_str) = line.strip_prefix("data: ") {
                                if let Ok(chunk) =
                                    serde_json::from_str::<openai::ChatCompletionChunk>(json_str)
                                {
                                    let events = translator.process_chunk(&chunk);
                                    if !send_events(&tx, &events).await {
                                        tracing::debug!("client disconnected during stream");
                                        return;
                                    }
                                }
                            }
                        }

                        // Remove processed frame from buffer in-place
                        let drain_to = pos + 2;
                        buffer.drain(..drain_to);
                    }
                }

                // Stream ended without [DONE]; still finish cleanly
                let events = translator.finish();
                send_events(&tx, &events).await;
                metrics.record_success();
            }
            Err(e) => {
                tracing::error!("streaming request failed: {e}");
                metrics.record_error();
                let err_event = anthropic::StreamEvent::Error {
                    error: anthropic::streaming::StreamError {
                        error_type: "api_error".to_string(),
                        message: format!("{e}"),
                    },
                };
                if let Ok(sse) = super::sse::stream_event_to_sse(&err_event) {
                    let _ = tx.send(Ok(sse)).await;
                }
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

async fn messages(
    State(state): State<AppState>,
    Json(body): Json<anthropic::MessageCreateRequest>,
) -> Response {
    state.metrics.record_request();

    if body.stream == Some(true) {
        let sse = messages_stream(state, body).await;
        return sse.into_response();
    }

    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
    openai_req.model = state.model_mapping.map_model(&openai_req.model);
    let original_model = body.model.clone();

    match state.openai_client.chat_completion(&openai_req).await {
        Ok((openai_resp, _status)) => {
            state.metrics.record_success();
            let anthropic_resp =
                mapping::message_map::openai_to_anthropic_response(&openai_resp, &original_model);
            (StatusCode::OK, Json(anthropic_resp)).into_response()
        }
        Err(OpenAIClientError::ApiError { status, error }) => {
            state.metrics.record_error();
            let anthropic_err =
                mapping::errors_map::openai_to_anthropic_error(&error, status, None);
            let http_status =
                StatusCode::from_u16(mapping::errors_map::anthropic_error_type_to_status(
                    &anthropic_err.error.error_type,
                ))
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (http_status, Json(anthropic_err)).into_response()
        }
        Err(e) => {
            state.metrics.record_error();
            tracing::error!("OpenAI client error: {e}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                format!("Upstream error: {e}"),
                None,
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}
