use crate::admin::state::{AdminEvent, RequestLogEntry, RuntimeConfig, SharedState};
use crate::backend::{BackendClient, BackendError};
use crate::config::{BackendKind, Config, MultiConfig};
use crate::metrics::Metrics;
use anthropic_openai_translate::{anthropic, mapping, openai};
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, FromRequest, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

use super::passthrough::anthropic_passthrough;
use super::streaming::messages_stream;
use super::token_counting::count_tokens;

/// Custom JSON extractor that returns Anthropic-shaped error responses on
/// parse failure. Axum's built-in Json returns its own error format, which
/// would break clients expecting Anthropic error shapes.
pub(crate) struct AnthropicJson<T>(pub T);

impl<S, T> FromRequest<S> for AnthropicJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AnthropicJson(value)),
            Err(rejection) => {
                let err = mapping::errors_map::create_anthropic_error(
                    anthropic::ErrorType::InvalidRequestError,
                    rejection.body_text(),
                    None,
                );
                Err((StatusCode::BAD_REQUEST, Json(err)).into_response())
            }
        }
    }
}

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
    /// Concurrency limiter. Uses try_acquire (fail-fast) instead of queueing
    /// to prevent cascading latency under load. Requests exceeding the limit
    /// get 429 immediately, matching Anthropic's rate limiting behavior.
    pub concurrency: Arc<Semaphore>,
    /// Strip `stream_options` from streaming requests for local LLM compat.
    pub omit_stream_options: bool,
}

impl AppState {
    /// Map a model name through the current runtime config for this backend.
    pub(crate) fn map_model(&self, model: &str) -> String {
        let config = self
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(mapping) = config.model_mappings.get(&self.backend_name) {
            mapping.map_model(model)
        } else {
            model.to_string()
        }
    }

    /// Whether request/response body logging is enabled.
    pub(crate) fn log_bodies(&self) -> bool {
        self.runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .log_bodies
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

    // Build per-backend sub-routers. Keep a map of AppState so the default
    // backend can reuse the same state (same semaphore, same reqwest client).
    let mut backend_states: HashMap<String, (AppState, bool)> = HashMap::new();
    for (name, bc) in &config.backends {
        let metrics = Metrics::new();
        backend_metrics.insert(name.clone(), metrics.clone());

        let state = AppState {
            backend: BackendClient::from_backend_config(bc),
            metrics,
            runtime_config: runtime_config.clone(),
            shared: shared.clone(),
            backend_name: name.clone(),
            concurrency: Arc::new(Semaphore::new(super::middleware::MAX_CONCURRENT_REQUESTS)),
            omit_stream_options: bc.omit_stream_options,
        };

        let is_anthropic = bc.kind == BackendKind::Anthropic;
        let sub = backend_router(state.clone(), is_anthropic);
        backend_states.insert(name.clone(), (state, is_anthropic));

        // Nest under /{name}/
        router = router.nest(&format!("/{name}"), sub);
    }

    // Default backend: also serve at un-prefixed /v1/messages for backward compat.
    // Reuses the same AppState (shared semaphore, connection pool) as the named route.
    if let Some((default_state, is_anthropic)) = backend_states.get(&config.default_backend) {
        let default_sub = backend_router(default_state.clone(), *is_anthropic);
        router = router.merge(default_sub);
    }

    let global_state = GlobalState {
        backend_metrics: Arc::new(backend_metrics),
    };

    // Metrics requires auth (prevents unauthenticated reconnaissance of
    // backend names and traffic patterns).
    let metrics_route = Router::new()
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
        .layer(axum::middleware::from_fn(super::middleware::validate_auth));

    // Health is public (no auth required).
    Router::new()
        .route("/health", get(health))
        .merge(metrics_route)
        .merge(router)
        .fallback(fallback_not_found)
        .layer(axum::middleware::from_fn(super::middleware::add_request_id))
        .with_state(global_state)
}

/// Return Anthropic-shaped 404 for any unmatched route (PRD US-004).
async fn fallback_not_found() -> Response {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::NotFoundError,
        "Not found".to_string(),
        None,
    );
    (StatusCode::NOT_FOUND, Json(err)).into_response()
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
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            enforce_concurrency,
        ))
        .with_state(state)
}

/// Reject requests when the concurrency limit is reached (429), rather than
/// queueing them like Tower's ConcurrencyLimitLayer would.
/// The permit is stored in request extensions so streaming handlers can hold
/// it until the stream completes (not just until headers are sent).
async fn enforce_concurrency(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Ok(permit) = state.concurrency.clone().try_acquire_owned() else {
        let err = mapping::errors_map::create_anthropic_error(
            anthropic::ErrorType::RateLimitError,
            "Proxy concurrency limit reached".to_string(),
            None,
        );
        return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
    };
    request.extensions_mut().insert(ConcurrencyPermit(Arc::new(permit)));
    next.run(request).await
}

/// Wrapper so OwnedSemaphorePermit can be stored in request extensions.
/// The field is never read directly; it exists as an RAII guard to hold
/// the permit until the struct is dropped.
#[derive(Clone)]
pub(crate) struct ConcurrencyPermit(#[allow(dead_code)] pub(crate) Arc<tokio::sync::OwnedSemaphorePermit>);

static MODELS_RESPONSE: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
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
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
    // Hold concurrency permit for streaming: passed to the spawned task so
    // the permit lives until the stream completes, not just until headers are sent.
    let permit = permit.map(|axum::Extension(p)| p);
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
        match messages_stream(state, body, ctx, mapped_model, permit).await {
            Ok((rate_limits, sse)) => {
                let mut response = sse.into_response();
                rate_limits.inject_anthropic_response_headers(response.headers_mut());
                return response;
            }
            Err(e) => {
                // Pre-stream backend error: return proper HTTP status instead of 200 OK
                return backend_error_to_response(e);
            }
        }
    }

    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            inject_gemini_thinking(&body, &state.backend, &mut openai_req);
            if state.omit_stream_options {
                openai_req.stream_options = None;
            }
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
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
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
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
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
pub(crate) struct RequestCtx {
    pub(crate) request_id: String,
    pub(crate) start: std::time::Instant,
    pub(crate) model_requested: String,
}

impl RequestCtx {
    /// Build a log entry, filling common fields from the context.
    pub(crate) fn log_entry(
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

/// When routing through the Gemini OpenAI-compatible endpoint, inject Anthropic's
/// thinking config into the `google` extension field that Gemini expects.
pub(crate) fn inject_gemini_thinking(
    body: &anthropic::MessageCreateRequest,
    backend: &BackendClient,
    req: &mut openai::ChatCompletionRequest,
) {
    if !matches!(
        backend,
        BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
    ) {
        return;
    }
    if let Some(anthropic::ThinkingConfig::Enabled { budget_tokens }) = &body.thinking {
        req.extra.insert(
            "google".to_string(),
            serde_json::json!({
                "thinking_config": { "thinking_budget": budget_tokens }
            }),
        );
    }
}

/// Log a completed request to the admin write buffer and broadcast to WebSocket clients.
pub(crate) fn log_request(shared: &Option<SharedState>, entry: RequestLogEntry) {
    if let Some(ref shared) = shared {
        let _ = shared
            .events_tx
            .send(AdminEvent::RequestCompleted(entry.clone()));
        let _ = shared.log_tx.try_send(entry);
    }
}
