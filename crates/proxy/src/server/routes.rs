use crate::admin::state::{AdminEvent, RequestLogEntry, RuntimeConfig, SharedState};
use crate::backend::{BackendClient, BackendError};
use crate::cache::{self, CacheBackend, CacheEntry, CacheNamespace};
use crate::config::{Config, MultiConfig};
use crate::metrics::Metrics;
use anyllm_translate::{anthropic, compute_request_warnings, mapping, openai};
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

use crate::batch::anthropic_batch;
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

/// Result of resolving a model name through the model router.
pub(crate) enum ResolvedModel {
    /// Routed via model_list to a specific backend and actual model name.
    Routed {
        backend_name: String,
        model: String,
        /// The deployment Arc for recording in-flight/latency stats.
        deployment: Arc<crate::config::model_router::Deployment>,
    },
    /// Model is known but all deployments are at their RPM limit.
    AllAtLimit,
    /// No model router, or model not in router. Used legacy ModelMapping.
    Legacy(String),
}

/// Per-backend state shared across request handlers.
///
/// In single-backend mode, one `AppState` serves all routes. In multi-backend mode,
/// each backend gets its own `AppState` mounted under a prefix path (e.g., `/openai/v1/messages`).
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
    /// Optional response cache for non-streaming requests.
    pub cache: Option<Arc<crate::cache::memory::MemoryCache>>,
    /// Model-level router for LiteLLM model_list configs. None for TOML/env configs.
    /// Wrapped in RwLock for dynamic model management via admin API.
    pub model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    /// All backend states, for cross-backend model routing. None unless model_router is set.
    pub all_backends: Option<Arc<HashMap<String, AppState>>>,
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

    /// Resolve a model name through the model router (if set) or fall back to ModelMapping.
    pub(crate) fn resolve_model(&self, model: &str) -> ResolvedModel {
        if let Some(ref router_lock) = self.model_router {
            let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
            if let Some(routed) = router.route(model) {
                return ResolvedModel::Routed {
                    backend_name: routed.backend_name.to_string(),
                    model: routed.actual_model.to_string(),
                    deployment: routed.deployment.clone(),
                };
            }
            if router.has_model(model) {
                return ResolvedModel::AllAtLimit;
            }
        }
        ResolvedModel::Legacy(self.map_model(model))
    }

    /// Resolve model and return (mapped_model, effective AppState, optional deployment).
    /// If the model routes to a different backend, the returned state is cloned from
    /// all_backends. Returns Err with a 429 response if all deployments are at limit.
    /// The deployment Arc is returned so handlers can call record_start/record_finish.
    #[allow(clippy::result_large_err)]
    pub(crate) fn resolve_model_and_state(
        &self,
        model: &str,
    ) -> Result<
        (
            String,
            AppState,
            Option<Arc<crate::config::model_router::Deployment>>,
        ),
        Response,
    > {
        match self.resolve_model(model) {
            ResolvedModel::Routed {
                backend_name,
                model: mapped,
                deployment,
            } => {
                let effective = self
                    .all_backends
                    .as_ref()
                    .and_then(|m| m.get(&backend_name))
                    .cloned()
                    .unwrap_or_else(|| self.clone());
                Ok((mapped, effective, Some(deployment)))
            }
            ResolvedModel::AllAtLimit => {
                let err = mapping::errors_map::create_anthropic_error(
                    anthropic::ErrorType::RateLimitError,
                    "all deployments for this model are at their RPM limit".to_string(),
                    None,
                );
                Err((StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response())
            }
            ResolvedModel::Legacy(mapped) => Ok((mapped, self.clone(), None)),
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
    app_multi_with_shared(config, None, None)
}

/// Build the axum router with optional shared admin state and model router.
pub fn app_multi_with_shared(
    config: MultiConfig,
    shared: Option<SharedState>,
    model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
) -> Router {
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

    // Build a shared cache instance for all backends.
    let cache_config = crate::cache::CacheConfig::from_env();
    let response_cache = Arc::new(crate::cache::memory::MemoryCache::new(&cache_config));

    // Build per-backend sub-routers. Keep a map of AppState so the default
    // backend can reuse the same state (same semaphore, same reqwest client).
    let mut backend_states: HashMap<String, (AppState, HandlerMode)> = HashMap::new();
    for (name, bc) in &config.backends {
        let metrics = Metrics::new();
        backend_metrics.insert(name.clone(), metrics.clone());

        let backend = BackendClient::from_backend_config(bc);
        let mode = match &backend {
            BackendClient::GeminiNative(_) => HandlerMode::GeminiNative,
            BackendClient::Anthropic(_) => HandlerMode::Anthropic,
            BackendClient::Bedrock(_) => HandlerMode::Bedrock,
            _ => HandlerMode::Translate,
        };

        let state = AppState {
            backend,
            metrics,
            runtime_config: runtime_config.clone(),
            shared: shared.clone(),
            backend_name: name.clone(),
            concurrency: Arc::new(Semaphore::new(super::middleware::MAX_CONCURRENT_REQUESTS)),
            omit_stream_options: bc.omit_stream_options,
            cache: Some(response_cache.clone()),
            model_router: model_router.clone(),
            // all_backends is set after the loop (needs all states built first).
            all_backends: None,
        };
        let sub = backend_router(state.clone(), mode);
        backend_states.insert(name.clone(), (state, mode));

        // Nest under /{name}/
        router = router.nest(&format!("/{name}"), sub);
    }

    // If a model router is active, build the all_backends map so handlers can
    // dispatch to a different backend when the router says so.
    if model_router.is_some() {
        let all_map: Arc<HashMap<String, AppState>> = Arc::new(
            backend_states
                .iter()
                .map(|(k, (s, _))| (k.clone(), s.clone()))
                .collect(),
        );
        // Patch each AppState in the map. Since we already built sub-routers with
        // the old states (all_backends=None), this only affects the default backend
        // and cross-backend routing lookups via effective_state(). The sub-router
        // states don't need all_backends because they are only reached by prefix.
        for (_, (state, _)) in backend_states.iter_mut() {
            state.all_backends = Some(all_map.clone());
        }
    }

    // Default backend: also serve at un-prefixed /v1/messages for backward compat.
    // Reuses the same AppState (shared semaphore, connection pool) as the named route.
    if let Some((default_state, mode)) = backend_states.get(&config.default_backend) {
        let default_sub = backend_router(default_state.clone(), *mode);
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
    let mut final_router = Router::new()
        .route("/health", get(health))
        .merge(metrics_route)
        .merge(router)
        .fallback(fallback_not_found)
        .layer(axum::middleware::from_fn(super::middleware::add_request_id));

    // Apply IP allowlist middleware before auth if IP_ALLOWLIST is configured.
    if super::middleware::ip_allowlist_active() {
        final_router = final_router.layer(axum::middleware::from_fn(
            super::middleware::check_ip_allowlist,
        ));
        tracing::info!("IP allowlist middleware enabled");
    }

    final_router.with_state(global_state)
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

/// Which handler mode a backend uses.
#[derive(Debug, Clone, Copy)]
enum HandlerMode {
    /// Anthropic passthrough (no translation, forwards raw bytes).
    Anthropic,
    /// Bedrock (SigV4 signing, event stream decoding, Anthropic format).
    Bedrock,
    /// Gemini native generateContent (no OpenAI translation layer).
    GeminiNative,
    /// Translation (Anthropic -> OpenAI -> backend -> OpenAI -> Anthropic).
    Translate,
}

/// Build the sub-router for a single backend.
fn backend_router(state: AppState, mode: HandlerMode) -> Router<GlobalState> {
    // Routes common to all backend modes.
    let common_routes: Router<AppState> = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/files", post(crate::batch::routes::upload_file))
        .route(
            "/v1/batches",
            post(crate::batch::routes::create_batch).get(crate::batch::routes::list_batches),
        )
        .route(
            "/v1/batches/{batch_id}",
            get(crate::batch::routes::get_batch),
        );

    let api_routes = match mode {
        HandlerMode::Anthropic => common_routes.route("/v1/messages", post(anthropic_passthrough)),
        HandlerMode::Bedrock => common_routes.route(
            "/v1/messages",
            post(super::bedrock_passthrough::bedrock_passthrough),
        ),
        HandlerMode::GeminiNative => common_routes.route(
            "/v1/messages",
            post(super::gemini_native::gemini_native_handler),
        ),
        HandlerMode::Translate => common_routes
            .route("/v1/messages", post(messages))
            .route(
                "/v1/chat/completions",
                post(super::chat_completions::chat_completions),
            )
            .route("/v1/messages/count_tokens", post(count_tokens))
            .route(
                "/v1/messages/batches",
                post(anthropic_batch::create_anthropic_batch),
            )
            .route(
                "/v1/messages/batches/{id}",
                get(anthropic_batch::get_anthropic_batch),
            )
            .route(
                "/v1/messages/batches/{id}/results",
                get(anthropic_batch::get_anthropic_batch_results),
            )
            .route("/v1/embeddings", post(embeddings))
            .route(
                "/v1/audio/transcriptions",
                post(super::audio::audio_transcriptions),
            )
            .route("/v1/audio/speech", post(super::audio::audio_speech))
            .route(
                "/v1/images/generations",
                post(super::images::image_generations),
            )
            .route("/v1/rerank", post(rerank))
            .route("/v1/completions", post(completions)),
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
    request
        .extensions_mut()
        .insert(ConcurrencyPermit(Arc::new(permit)));
    next.run(request).await
}

/// Wrapper so OwnedSemaphorePermit can be stored in request extensions.
/// The field is never read directly; it exists as an RAII guard to hold
/// the permit until the struct is dropped.
#[derive(Clone)]
pub(crate) struct ConcurrencyPermit(
    #[allow(dead_code)] pub(crate) Arc<tokio::sync::OwnedSemaphorePermit>,
);

/// Static Claude model entries, merged with model_list models at runtime.
static STATIC_CLAUDE_MODELS: std::sync::LazyLock<Vec<serde_json::Value>> = std::sync::LazyLock::new(
    || {
        vec![
            // Claude 4.x
            serde_json::json!({"id": "claude-opus-4-6",            "object": "model", "created": 1715644800, "owned_by": "anthropic", "display_name": "Claude Opus 4.6"}),
            serde_json::json!({"id": "claude-sonnet-4-6",          "object": "model", "created": 1715644800, "owned_by": "anthropic", "display_name": "Claude Sonnet 4.6"}),
            serde_json::json!({"id": "claude-opus-4-5",            "object": "model", "created": 1715644800, "owned_by": "anthropic", "display_name": "Claude Opus 4.5"}),
            serde_json::json!({"id": "claude-sonnet-4-5",          "object": "model", "created": 1715644800, "owned_by": "anthropic", "display_name": "Claude Sonnet 4.5"}),
            serde_json::json!({"id": "claude-haiku-4-5",           "object": "model", "created": 1715644800, "owned_by": "anthropic", "display_name": "Claude Haiku 4.5"}),
            serde_json::json!({"id": "claude-haiku-4-5-20251001",  "object": "model", "created": 1727740800, "owned_by": "anthropic", "display_name": "Claude Haiku 4.5 (Oct 2025)"}),
            // Claude 3.7
            serde_json::json!({"id": "claude-3-7-sonnet-20250219", "object": "model", "created": 1708300800, "owned_by": "anthropic", "display_name": "Claude 3.7 Sonnet"}),
            // Claude 3.5
            serde_json::json!({"id": "claude-3-5-sonnet-20241022", "object": "model", "created": 1729555200, "owned_by": "anthropic", "display_name": "Claude 3.5 Sonnet (Oct 2024)"}),
            serde_json::json!({"id": "claude-3-5-sonnet-20240620", "object": "model", "created": 1718841600, "owned_by": "anthropic", "display_name": "Claude 3.5 Sonnet (Jun 2024)"}),
            serde_json::json!({"id": "claude-3-5-haiku-20241022",  "object": "model", "created": 1729555200, "owned_by": "anthropic", "display_name": "Claude 3.5 Haiku"}),
            // Claude 3
            serde_json::json!({"id": "claude-3-opus-20240229",     "object": "model", "created": 1709164800, "owned_by": "anthropic", "display_name": "Claude 3 Opus"}),
            serde_json::json!({"id": "claude-3-sonnet-20240229",   "object": "model", "created": 1709164800, "owned_by": "anthropic", "display_name": "Claude 3 Sonnet"}),
            serde_json::json!({"id": "claude-3-haiku-20240307",    "object": "model", "created": 1709769600, "owned_by": "anthropic", "display_name": "Claude 3 Haiku"}),
        ]
    },
);

/// GET /v1/models -- returns static Claude models merged with model_list entries.
async fn models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut data: Vec<serde_json::Value> = STATIC_CLAUDE_MODELS.clone();

    // Merge models from the model router (LiteLLM model_list config).
    if let Some(ref router_lock) = state.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        let static_ids: std::collections::HashSet<String> = data
            .iter()
            .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
            .collect();
        for model_name in router.known_models() {
            if !static_ids.contains(model_name) {
                data.push(serde_json::json!({
                    "id": model_name,
                    "object": "model",
                    "created": 0,
                    "owned_by": "organization"
                }));
            }
        }
    }

    Json(serde_json::json!({
        "object": "list",
        "data": data,
    }))
}

async fn health() -> impl IntoResponse {
    ([("content-type", "application/json")], r#"{"status":"ok"}"#)
}

/// Convert a BackendError into an Anthropic error Response.
pub(super) fn backend_error_to_response(error: BackendError) -> Response {
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

/// Return the appropriate `x-anyllm-cache` header value.
pub(crate) fn cache_header_value(bypass: bool) -> axum::http::HeaderValue {
    if bypass {
        axum::http::HeaderValue::from_static("bypass")
    } else {
        axum::http::HeaderValue::from_static("miss")
    }
}

/// Store a serializable response in the cache if caching is enabled.
pub(crate) async fn try_cache_response<T: serde::Serialize>(
    cache_key: &Option<String>,
    cache: &Option<Arc<crate::cache::memory::MemoryCache>>,
    cache_ttl: Option<u64>,
    response: &T,
    model: String,
) {
    if let (Some(ref key), Some(ref c)) = (cache_key, cache) {
        if let Ok(resp_body) = serde_json::to_vec(response).map(bytes::Bytes::from) {
            let ttl = cache_ttl.unwrap_or(c.default_ttl_secs);
            c.put(
                key,
                CacheEntry {
                    response_body: resp_body,
                    model,
                    created_at: std::time::Instant::now(),
                    ttl_secs: cache_ttl,
                },
                ttl,
            )
            .await;
        }
    }
}

/// Inject degradation warnings as `x-anyllm-degradation` header if any features were dropped.
pub(crate) fn inject_degradation_header(
    headers: &mut axum::http::HeaderMap,
    warnings: &anyllm_translate::TranslationWarnings,
) {
    if let Some(val) = warnings.as_header_value() {
        if let Ok(hv) = axum::http::HeaderValue::from_str(&val) {
            headers.insert("x-anyllm-degradation", hv);
        }
    }
}

/// Shared passthrough logic: extract content-type, forward to backend, relay response.
async fn passthrough_to_backend(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    body: axum::body::Bytes,
    path: &str,
) -> Response {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    match state
        .backend
        .raw_passthrough(path, body, content_type)
        .await
    {
        Ok((status, resp_headers, resp_body)) => {
            let mut response = (status, resp_body).into_response();
            for (k, v) in &resp_headers {
                response.headers_mut().insert(k, v.clone());
            }
            response
        }
        Err(e) => backend_error_to_response(e),
    }
}

async fn embeddings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    passthrough_to_backend(&state, &headers, body, "/v1/embeddings").await
}

async fn rerank(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    passthrough_to_backend(&state, &headers, body, "/v1/rerank").await
}

async fn completions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    passthrough_to_backend(&state, &headers, body, "/v1/completions").await
}

async fn messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<super::middleware::VirtualKeyContext>>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
    // Hold concurrency permit for streaming: passed to the spawned task so
    // the permit lives until the stream completes, not just until headers are sent.
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
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

    // Enforce model allowlist policy for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        if !super::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", body.model),
                None,
            );
            return (StatusCode::FORBIDDEN, Json(err)).into_response();
        }
    }

    if state.log_bodies() {
        tracing::debug!(
            model = %body.model,
            stream = ?body.stream,
            message_count = body.messages.len(),
            body = %serde_json::to_string(&body).unwrap_or_else(|_| "[serialization failed]".into()),
            "request body"
        );
    }

    let warnings = compute_request_warnings(&body);

    let is_streaming = body.stream == Some(true);

    if is_streaming {
        if state.log_bodies() {
            tracing::debug!(model = %body.model, "streaming request initiated");
        }
        let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&body.model)
        {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        if let Some(ref d) = deployment {
            d.record_start();
        }
        // Logging deferred until stream completes (inside messages_stream tasks).
        let stream_start = std::time::Instant::now();
        match messages_stream(effective, body, ctx, mapped_model, permit, vk_ctx.clone()).await {
            Ok((rate_limits, sse)) => {
                // For streaming, record_finish is approximate (headers sent, not stream end).
                if let Some(ref d) = deployment {
                    d.record_finish(stream_start.elapsed().as_millis() as u64);
                }
                let mut response = sse.into_response();
                rate_limits.inject_anthropic_response_headers(response.headers_mut());
                inject_degradation_header(response.headers_mut(), &warnings);
                response.headers_mut().insert(
                    "x-anyllm-cache",
                    axum::http::HeaderValue::from_static("bypass"),
                );
                return response;
            }
            Err(e) => {
                if let Some(ref d) = deployment {
                    d.record_finish(stream_start.elapsed().as_millis() as u64);
                }
                // Pre-stream backend error: return proper HTTP status instead of 200 OK
                return backend_error_to_response(e);
            }
        }
    }

    // Non-streaming: check cache before calling backend.
    let body_value = serde_json::to_value(&body).unwrap_or_default();
    let cache_ttl = match cache::parse_cache_ttl(&body_value) {
        Ok(ttl) => ttl,
        Err(msg) => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                msg,
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };
    let bypass_cache = cache_ttl == Some(0);
    let cache_key = if !bypass_cache {
        Some(cache::cache_key_for_request(
            &body_value,
            CacheNamespace::Anthropic,
        ))
    } else {
        None
    };

    // Check cache on non-bypass requests
    if let (Some(ref key), Some(ref c)) = (&cache_key, &state.cache) {
        if let Some(entry) = c.get(key).await {
            tracing::debug!(cache_key = %key, "cache hit for /v1/messages");
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("x-anyllm-cache", "hit")
                .body(axum::body::Body::from(entry.response_body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            inject_degradation_header(response.headers_mut(), &warnings);
            return response;
        }
    }

    // Resolve model routing (may switch to a different backend).
    let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&body.model) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Some(ref d) = deployment {
        d.record_start();
    }
    let backend_start = std::time::Instant::now();

    match &effective.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            inject_gemini_thinking(&body, &effective.backend, &mut openai_req);
            // Gemini/Vertex rejects standard JSON Schema keywords; sanitize tool schemas.
            if matches!(
                effective.backend,
                BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
            ) {
                if let Some(tools) = openai_req.tools.take() {
                    openai_req.tools = Some(
                        tools
                            .into_iter()
                            .map(|mut t| {
                                if let Some(params) = t.function.parameters.take() {
                                    t.function.parameters = Some(
                                        mapping::tools_map::sanitize_schema_for_gemini(params),
                                    );
                                }
                                t
                            })
                            .collect(),
                    );
                }
            }
            if effective.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let mapped_model = openai_req.model.clone();
            let original_model = body.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
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
                    record_vk_tpm(&vk_ctx, anthropic_resp.usage.output_tokens);
                    let cost = crate::cost::record_cost(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );

                    try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &anthropic_resp,
                        original_model,
                    )
                    .await;

                    let cache_hv = cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = mapped_model.clone();
            let mapped_model = responses_req.model.clone();
            let original_model = body.model.clone();

            match client.responses(&responses_req).await {
                Ok((resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
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
                    record_vk_tpm(&vk_ctx, anthropic_resp.usage.output_tokens);
                    let cost = crate::cost::record_cost(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                    try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &anthropic_resp,
                        original_model,
                    )
                    .await;

                    let cache_hv = cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    backend_error_to_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Anthropic(_) | BackendClient::Bedrock(_) | BackendClient::GeminiNative(_) => {
            // These backends are handled by separate handlers (passthrough / Bedrock / Gemini native).
            // If we reach here, something is misconfigured.
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "This backend does not use the translation handler".to_string(),
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
            key_id: None,
            cost_usd: None,
        }
    }

    /// Build a log entry with attribution (key_id from virtual key, cost from pricing).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn log_entry_with_attribution(
        &self,
        backend_name: &str,
        model_mapped: Option<String>,
        status_code: u16,
        tokens: Option<(u64, u64)>,
        is_streaming: bool,
        error_message: Option<String>,
        vk_ctx: &Option<super::middleware::VirtualKeyContext>,
        cost_usd: Option<f64>,
    ) -> RequestLogEntry {
        let mut entry = self.log_entry(
            backend_name,
            model_mapped,
            status_code,
            tokens,
            is_streaming,
            error_message,
        );
        entry.key_id = vk_ctx.as_ref().map(|ctx| ctx.key_id);
        // Only store non-zero costs.
        entry.cost_usd = cost_usd.filter(|&c| c > 0.0);
        entry
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

/// Record output tokens against the virtual key's TPM sliding window.
/// Called after the backend response is received and token count is known.
pub(crate) fn record_vk_tpm(
    vk_ctx: &Option<super::middleware::VirtualKeyContext>,
    output_tokens: u32,
) {
    if let Some(ctx) = vk_ctx {
        ctx.rate_state
            .record_tpm(crate::admin::keys::now_ms(), output_tokens);
    }
}

/// Global webhook callback config, set once at startup.
static CALLBACKS: std::sync::OnceLock<Arc<crate::callbacks::CallbackConfig>> =
    std::sync::OnceLock::new();

/// Set the global webhook callback config (called once at startup).
pub fn set_callbacks(config: Arc<crate::callbacks::CallbackConfig>) {
    let _ = CALLBACKS.set(config);
}

/// Get a reference to the global webhook callback config, if set.
pub fn get_callbacks() -> Option<&'static Arc<crate::callbacks::CallbackConfig>> {
    CALLBACKS.get()
}

/// Log a completed request to the admin write buffer, broadcast to WebSocket clients,
/// and fire webhook callbacks if configured.
pub(crate) fn log_request(shared: &Option<SharedState>, entry: RequestLogEntry) {
    if let Some(cb) = CALLBACKS.get() {
        cb.notify(&entry);
    }
    if let Some(ref shared) = shared {
        let _ = shared
            .events_tx
            .send(AdminEvent::RequestCompleted(entry.clone()));
        let _ = shared.log_tx.try_send(entry);
    }
}
