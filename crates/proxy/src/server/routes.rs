use crate::admin::state::{RuntimeConfig, SharedState};
use crate::backend::BackendClient;
use crate::config::{Config, MultiConfig};
use crate::metrics::Metrics;
use crate::server::state::{AppState, ConcurrencyPermit, GlobalState, ToolEngineState};
use anyllm_providers::ProviderCatalog;
use anyllm_translate::{anthropic, mapping};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

use super::passthrough::anthropic_passthrough;
use crate::batch::anthropic_batch;

pub mod context;
pub mod handlers;
pub mod helpers;
pub mod messages;
#[cfg(test)]
mod tests;

pub(crate) use context::RequestCtx;
pub(crate) use context::{inject_gemini_thinking, inject_glm_thinking};
pub(super) use helpers::backend_error_to_response;
pub(crate) use helpers::{
    cache_auth_identity, cache_header_value, inject_degradation_header, log_request,
    record_virtual_key_usage, set_backend_error_kind, try_cache_response,
};
pub use helpers::{get_callbacks, set_callbacks};

use handlers::{completions, embeddings, health, models, rerank, v2_rerank};
use messages::messages;

/// Build the axum router from a legacy single-backend Config.
pub fn app(config: Config) -> Router {
    let multi = MultiConfig::from_single_config(&config);
    app_multi(multi)
}

/// Build the axum router from multi-backend configuration.
/// Creates nested sub-routers for each configured backend.
pub fn app_multi(config: MultiConfig) -> Router {
    app_multi_with_shared(config, None, None, None, None, None)
}

/// Build the axum router with optional shared admin state and model router.
pub fn app_multi_with_shared(
    config: MultiConfig,
    shared: Option<SharedState>,
    model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    tool_engine: Option<Arc<ToolEngineState>>,
    batch_engine: Option<
        Arc<
            anyllm_batch_engine::BatchEngine<
                anyllm_batch_engine::queue::sqlite::SqliteQueue,
                anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue,
            >,
        >,
    >,
    admin_port: Option<u16>,
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
            redact_secrets: config.redact_secrets,
            anthropic_thinking_repair: config.anthropic_thinking_repair,
            forward_client_auth: config.forward_client_auth,
            // Derive from the static tool-engine preset (built from YAML/env
            // at startup) rather than hardcoding Disabled -- otherwise a
            // standalone deployment (no --webui/--admin, so `shared` is None)
            // silently disables guardrails on the non-streaming path while
            // the streaming path still honors `engine.guardrails`. Mirrors
            // the derivation in main_helpers/async_main/admin.rs.
            tool_guardrail_mode: tool_engine
                .as_ref()
                .map(|e| e.guardrails.mode)
                .unwrap_or(crate::tools::ToolGuardrailMode::Disabled)
                .as_str()
                .to_string(),
        }))
    };

    // Build a shared cache instance for all backends.
    let cache_config = crate::cache::CacheConfig::from_env();
    let response_cache = Arc::new(crate::cache::memory::MemoryCache::new(&cache_config));

    // Anthropic thinking-block repair store (Anthropic-passthrough only).
    // Shared across all Anthropic-mode backends, same pattern as
    // `response_cache` above. Always constructed for Anthropic-mode backends
    // (cheap: empty moka caches, see ThinkingRepairStore::new()) regardless of
    // whether the feature is enabled -- actual repair/record/commit behavior
    // is gated live per-request via RuntimeConfig.anthropic_thinking_repair
    // (AppState::thinking_repair_enabled()), so it's toggleable from the
    // admin UI without restart. Non-Anthropic backends still get `None` below.
    let thinking_repair_store = Some(Arc::new(crate::thinking_repair::ThinkingRepairStore::new()));

    let provider_catalog = shared
        .as_ref()
        .map(|s| s.provider_catalog.clone())
        .unwrap_or_else(|| Arc::new(ProviderCatalog::bundled()));

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
            provider_id: bc.provider_id.clone(),
            concurrency: Arc::new(Semaphore::new(super::middleware::MAX_CONCURRENT_REQUESTS)),
            omit_stream_options: bc.omit_stream_options,
            stream_timeout_secs: bc.stream_timeout_secs,
            expose_degradation_warnings: config.expose_degradation_warnings,
            cache: Some(response_cache.clone()),
            thinking_repair: if matches!(mode, HandlerMode::Anthropic) {
                thinking_repair_store.clone()
            } else {
                None
            },
            model_router: model_router.clone(),
            provider_catalog: provider_catalog.clone(),
            // all_backends is set after the loop (needs all states built first).
            all_backends: None,
            tool_engine: tool_engine.clone(),
            batch_engine: batch_engine.clone(),
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

    let final_router = final_router.with_state(global_state);

    // When admin UI is active, redirect proxy root to the admin UI. Uses the
    // incoming Host header so the hostname matches what the user typed.
    if let Some(port) = admin_port {
        Router::new()
            .route(
                "/",
                get(move |headers: axum::http::HeaderMap| async move {
                    let host = headers
                        .get("host")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|h| h.split(':').next())
                        .unwrap_or("localhost")
                        .to_owned();
                    axum::response::Redirect::temporary(&format!("http://{}:{}/admin/", host, port))
                }),
            )
            .merge(final_router)
    } else {
        final_router
    }
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
        )
        .route(
            "/v1/batches/{batch_id}/cancel",
            post(crate::batch::routes::cancel_batch),
        );

    // Gemini CLI input routes: accept native generateContent/streamGenerateContent format
    // on every backend. The handler translates to Anthropic format internally.
    let gemini_input_routes: Router<AppState> = Router::new().route(
        "/v1beta/models/{model_action}",
        post(super::gemini_input::gemini_input_handler),
    );

    let api_routes = match mode {
        HandlerMode::Anthropic => common_routes
            .route(
                "/v1/chat/completions",
                post(super::chat_completions::chat_completions),
            )
            .route("/v1/messages", post(anthropic_passthrough))
            // Catch-all for batch, file CRUD, and other Anthropic-native endpoints.
            // /v1/messages above takes priority (exact match beats wildcard).
            .route(
                "/v1/{*path}",
                axum::routing::any(super::passthrough::anthropic_generic_passthrough),
            )
            .merge(gemini_input_routes),
        HandlerMode::Bedrock => common_routes
            .route(
                "/v1/messages",
                post(super::bedrock_passthrough::bedrock_passthrough),
            )
            // Bedrock native endpoints: accept Bedrock-native JSON, proxy handles SigV4.
            // Client path: POST /{backend_name}/model/{modelId}/converse (or the default backend path)
            .route(
                "/model/{model_id}/converse",
                post(super::bedrock_native::bedrock_converse),
            )
            .route(
                "/model/{model_id}/converse-stream",
                post(super::bedrock_native::bedrock_converse_stream),
            )
            .route(
                "/model/{model_id}/invoke",
                post(super::bedrock_native::bedrock_invoke),
            )
            .route(
                "/model/{model_id}/invoke-with-response-stream",
                post(super::bedrock_native::bedrock_invoke_stream),
            )
            .merge(gemini_input_routes),
        HandlerMode::GeminiNative => common_routes
            .route(
                "/v1/messages",
                post(super::gemini_native::gemini_native_handler),
            )
            .merge(gemini_input_routes),
        HandlerMode::Translate => common_routes
            .route("/v1/messages", post(messages))
            .route(
                "/v1/chat/completions",
                post(super::chat_completions::chat_completions),
            )
            .route(
                "/v1/messages/count_tokens",
                post(super::token_counting::count_tokens),
            )
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
            .route("/v2/rerank", post(v2_rerank))
            .route("/v1/completions", post(completions))
            // Catch-all for any /v1/* path without an explicit handler.
            // Explicit routes above take priority; this fires only for unmatched paths.
            // Covers: /v1/responses, /v1/moderations, /v1/images/edits, /v1/images/variations,
            //         /v1/videos, /v1/fine_tuning/*, /v1/evals/*, /v1/assistants/*,
            //         /v1/threads/*, /v1/containers/*, /v1/vector_stores/*, files CRUD, etc.
            .route(
                "/v1/{*path}",
                axum::routing::any(super::generic_passthrough::v1_generic_passthrough),
            )
            .merge(gemini_input_routes),
    };

    api_routes
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            enforce_route_scope,
        ))
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

async fn enforce_route_scope(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let allowed_routes = request
        .extensions()
        .get::<super::middleware::VirtualKeyContext>()
        .and_then(|ctx| ctx.allowed_routes.clone());

    if allowed_routes.is_some() {
        if let Err(error) =
            super::policy::enforce_route_scope(&state.backend_name, &state.shared, &allowed_routes)
                .await
        {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                error.message().to_string(),
                None,
            );
            return (StatusCode::FORBIDDEN, Json(err)).into_response();
        }
    }

    next.run(request).await
}
