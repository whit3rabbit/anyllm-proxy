// Shared state types for request handlers: AppState, AnthropicJson, ResolvedModel, etc.
// Extracted from routes.rs so consumers can import state independently of the router setup.

use crate::admin::state::{RuntimeConfig, SharedState};
use crate::backend::BackendClient;
use crate::metrics::Metrics;
use anyllm_translate::{anthropic, mapping};
use axum::{
    extract::{rejection::JsonRejection, FromRequest},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

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

/// Shared state for tool execution, stored in AppState.
#[derive(Clone)]
pub struct ToolEngineState {
    pub registry: Arc<crate::tools::ToolRegistry>,
    pub policy: Arc<crate::tools::ToolExecutionPolicy>,
    pub loop_config: crate::tools::LoopConfig,
    pub mcp_manager: Option<Arc<crate::tools::McpServerManager>>,
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
    /// Wall-clock cap for streaming responses in seconds. 0 = disabled.
    /// Prevents resource exhaustion from stalled backends.
    pub stream_timeout_secs: u64,
    /// When true, set `x-anyllm-degradation` header on responses that silently drop features.
    /// Mirrors Config::expose_degradation_warnings / MultiConfig::expose_degradation_warnings.
    pub expose_degradation_warnings: bool,
    /// Optional response cache for non-streaming requests.
    pub cache: Option<Arc<crate::cache::memory::MemoryCache>>,
    /// Model-level router for LiteLLM model_list configs. None for TOML/env configs.
    /// Wrapped in RwLock for dynamic model management via admin API.
    pub model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    /// All backend states, for cross-backend model routing. None unless model_router is set.
    pub all_backends: Option<Arc<HashMap<String, AppState>>>,
    /// Tool execution engine state. None when tool execution is not configured.
    pub tool_engine: Option<Arc<ToolEngineState>>,
    /// Batch orchestration engine. None in test configs that don't need batch.
    pub batch_engine: Option<
        Arc<
            anyllm_batch_engine::BatchEngine<
                anyllm_batch_engine::queue::sqlite::SqliteQueue,
                anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue,
            >,
        >,
    >,
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
pub(crate) struct GlobalState {
    pub(crate) backend_metrics: Arc<HashMap<String, Metrics>>,
}

/// Wrapper so OwnedSemaphorePermit can be stored in request extensions.
/// The field is never read directly; it exists as an RAII guard to hold
/// the permit until the struct is dropped.
#[derive(Clone)]
pub(crate) struct ConcurrencyPermit(
    #[allow(dead_code)] pub(crate) Arc<tokio::sync::OwnedSemaphorePermit>,
);
