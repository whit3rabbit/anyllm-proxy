// Shared state types for request handlers: AppState, AnthropicJson, ResolvedModel, etc.
// Extracted from routes.rs so consumers can import state independently of the router setup.

use crate::admin::state::{RuntimeConfig, SharedState};
use crate::backend::BackendClient;
use crate::metrics::Metrics;
use anyllm_providers::ProviderCatalog;
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
        /// Per-route option overrides when routed via a DB route; `None` when
        /// routed via the LiteLLM model_router (inherit global config).
        options: Option<Arc<crate::config::route_router::RouteOptions>>,
    },
    /// Model is known but all deployments are at their RPM limit.
    AllAtLimit,
    /// Model router is active but the model alias is not configured.
    UnknownModel,
    /// No model router, or model not in router. Used legacy ModelMapping.
    Legacy(String),
}

/// Shared state for tool execution, stored in AppState.
#[derive(Clone)]
pub struct ToolEngineState {
    pub registry: Arc<crate::tools::ToolRegistry>,
    pub policy: Arc<crate::tools::ToolExecutionPolicy>,
    pub loop_config: crate::tools::LoopConfig,
    pub guardrails: crate::tools::ToolGuardrailConfig,
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
    /// Runtime config (model mappings, body logging, redaction) read on every request.
    /// Shared with admin server so config changes take effect immediately.
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    /// Shared admin state for request logging and live updates. None in tests.
    pub shared: Option<SharedState>,
    /// Per-route option overrides for the request that produced this (cloned)
    /// state, set by `resolve_model_and_state` when a DB route was selected.
    /// `None` means "no route override; use the global RuntimeConfig value".
    /// Read by the option accessors (`redact_secrets`, `effective_tool_guardrails`,
    /// `active_pxpipe`, `pxpipe_models`).
    pub route_options: Option<Arc<crate::config::route_router::RouteOptions>>,
    /// Backend name for logging purposes.
    pub backend_name: String,
    /// Canonical provider id used for provider/model policy decisions.
    pub provider_id: Option<String>,
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
    /// Anthropic thinking-block record-and-restore repair store. `None`
    /// unless `backend` is `BackendClient::Anthropic`; only consulted by
    /// `anthropic_passthrough`. Always `Some` for Anthropic backends
    /// regardless of whether the feature is enabled -- use
    /// `thinking_repair_enabled()` to check the live toggle before using it.
    pub thinking_repair: Option<Arc<crate::thinking_repair::ThinkingRepairStore>>,
    /// Text-to-image context compression engine (pxpipe). `None` unless
    /// `backend` is `BackendClient::Anthropic`; only consulted by
    /// `anthropic_passthrough`. Always `Some` for Anthropic backends regardless
    /// of the live toggle -- use `active_pxpipe()`, which checks
    /// `RuntimeConfig.pxpipe_compress`, before using it.
    pub pxpipe: Option<Arc<crate::pxpipe::PxpipeEngine>>,
    /// Model-level router for LiteLLM model_list configs. None for TOML/env configs.
    /// Wrapped in RwLock for dynamic model management via admin API.
    pub model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    /// Immutable provider/model catalog used for runtime model metadata.
    pub provider_catalog: Arc<ProviderCatalog>,
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

    /// Resolve a model name to a backend.
    ///
    /// Precedence: (1) admin-DB routes (`RouteRouter`), (2) LiteLLM model_router,
    /// (3) legacy ModelMapping. An empty route router falls straight through so
    /// installs without routes behave exactly as before.
    pub(crate) fn resolve_model(&self, model: &str) -> ResolvedModel {
        if let Some(shared) = self.shared.as_ref() {
            if let Some(ref rr_lock) = shared.route_router {
                use crate::config::route_router::RouteResolution;
                let rr = rr_lock.read().unwrap_or_else(|e| e.into_inner());
                if !rr.is_empty() {
                    match rr.resolve(model) {
                        RouteResolution::Routed(res) => {
                            return ResolvedModel::Routed {
                                backend_name: res.backend_name,
                                model: res.model,
                                deployment: res.deployment,
                                options: Some(res.options),
                            };
                        }
                        RouteResolution::AllAtLimit => return ResolvedModel::AllAtLimit,
                        // No route serves this model: fall through to the layers below.
                        RouteResolution::NoRoute => {}
                    }
                }
            }
        }
        if let Some(ref router_lock) = self.model_router {
            let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
            if let Some(routed) = router.route(model) {
                return ResolvedModel::Routed {
                    backend_name: routed.backend_name.to_string(),
                    model: routed.actual_model.to_string(),
                    deployment: routed.deployment.clone(),
                    options: None,
                };
            }
            if router.has_model(model) {
                return ResolvedModel::AllAtLimit;
            }
            return ResolvedModel::UnknownModel;
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
                options,
            } => {
                let mut effective = self
                    .all_backends
                    .as_ref()
                    .and_then(|m| m.get(&backend_name))
                    .cloned()
                    .or_else(|| {
                        // Check managed backends (SQLite-backed, zero-restart)
                        self.shared.as_ref().and_then(|s| {
                            let guard = s.managed_backends
                                .read()
                                .ok()
                                .or_else(|| {
                                    tracing::warn!("managed_backends RwLock is poisoned; skipping managed backend lookup");
                                    None
                                })?;
                            guard.get(&backend_name).map(|(row, client)| {
                                let mut state = self.clone();
                                state.backend = client.clone();
                                state.backend_name = backend_name.clone();
                                state.provider_id = Some(row.provider_id.clone());
                                state
                            })
                        })
                    })
                    .unwrap_or_else(|| self.clone());
                // Carry the per-route option overrides onto the effective state so
                // the option accessors resolve route-first, global-fallback.
                effective.route_options = options;
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
            ResolvedModel::UnknownModel => {
                let err = mapping::errors_map::create_anthropic_error(
                    anthropic::ErrorType::InvalidRequestError,
                    format!("model '{model}' is not configured in model_list"),
                    None,
                );
                Err((StatusCode::BAD_REQUEST, Json(err)).into_response())
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

    /// Whether upstream JSON/text request payloads should be redacted.
    /// Route override (if set) wins over the global RuntimeConfig value.
    pub(crate) fn redact_secrets(&self) -> bool {
        if let Some(v) = self.route_options.as_ref().and_then(|o| o.redact_secrets) {
            return v;
        }
        self.runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .redact_secrets
    }

    /// Effective tool-call guardrail config for this request: the runtime,
    /// admin-tunable override (`RuntimeConfig.tool_guardrail_mode`, no
    /// restart required) applied on top of `engine.guardrails` (the static
    /// preset built from YAML/env at startup). See
    /// `crate::tools::resolve_runtime_guardrails`.
    pub(crate) fn effective_tool_guardrails(
        &self,
        engine: &ToolEngineState,
    ) -> crate::tools::ToolGuardrailConfig {
        // Route override (if set) wins over the live global RuntimeConfig mode.
        if let Some(mode) = self
            .route_options
            .as_ref()
            .and_then(|o| o.guardrail_mode.as_deref())
        {
            return crate::tools::resolve_runtime_guardrails(&engine.guardrails, mode);
        }
        crate::tools::resolve_runtime_guardrails_locked(&self.runtime_config, &engine.guardrails)
    }

    /// Whether Anthropic thinking-block repair (record + restore) is active.
    /// `self.thinking_repair` may be `Some` even when this is `false` -- the
    /// store is always constructed for Anthropic backends; only this flag
    /// gates whether it's actually used.
    pub(crate) fn thinking_repair_enabled(&self) -> bool {
        self.runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .anthropic_thinking_repair
    }

    /// Whether Anthropic passthrough forwards the client's own incoming
    /// credential upstream instead of the operator's (`ANTHROPIC_FORWARD_CLIENT_AUTH`,
    /// live-toggleable via `RuntimeConfig.forward_client_auth`). Read fresh on
    /// every request -- unlike the old frozen `AppState` field this replaced,
    /// this reflects an admin-UI change immediately without a restart, and
    /// applies uniformly to every `BackendKind::Anthropic` backend since they
    /// all share one `RuntimeConfig`.
    pub(crate) fn forward_client_auth_enabled(&self) -> bool {
        self.runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .forward_client_auth
    }

    /// The thinking-repair store, but only when the live admin-toggleable
    /// flag is actually on. `None` both when repair is entirely absent (non-
    /// Anthropic backend) and when it's present-but-disabled -- single
    /// accessor so call sites collapse to `if let Some(store) = ...` instead
    /// of separately checking `thinking_repair_enabled()` and
    /// `thinking_repair.is_some()`.
    pub(crate) fn active_thinking_repair(
        &self,
    ) -> Option<Arc<crate::thinking_repair::ThinkingRepairStore>> {
        if self.thinking_repair_enabled() {
            self.thinking_repair.clone()
        } else {
            None
        }
    }

    /// The pxpipe compression engine, but only when the live admin-toggleable
    /// flag (`RuntimeConfig.pxpipe_compress`) is on. `None` both when the engine
    /// is absent (non-Anthropic backend) and when present-but-disabled.
    pub(crate) fn active_pxpipe(&self) -> Option<Arc<crate::pxpipe::PxpipeEngine>> {
        let enabled = match self.route_options.as_ref().and_then(|o| o.pxpipe_compress) {
            Some(v) => v,
            None => {
                self.runtime_config
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .pxpipe_compress
            }
        };
        if enabled {
            self.pxpipe.clone()
        } else {
            None
        }
    }

    /// Live model-scope CSV for pxpipe. Route override wins over the global
    /// `RuntimeConfig.pxpipe_models` value.
    pub(crate) fn pxpipe_models(&self) -> String {
        if let Some(csv) = self
            .route_options
            .as_ref()
            .and_then(|o| o.pxpipe_models.clone())
        {
            return csv;
        }
        self.runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .pxpipe_models
            .clone()
    }

    /// Vision gate: if the catalog knows this model and says it is NOT
    /// vision-capable, refuse (fail-closed). Unknown models fall back to the
    /// scope list only — a Claude passthrough model is vision-capable in
    /// practice, and the scope list is the operator's explicit control.
    fn pxpipe_vision_ok(&self, model: &str) -> bool {
        match self
            .provider_id
            .as_deref()
            .and_then(|pid| self.provider_catalog.get_model(pid, model))
        {
            Some(def) => def.capabilities.vision,
            None => true,
        }
    }

    /// The pxpipe engine for `model`, or `None` if compression shouldn't run:
    /// the master toggle is off, the engine is absent (non-Anthropic backend),
    /// the model is out of the live scope CSV, or it isn't vision-capable.
    /// Single accessor so `passthrough` collapses to
    /// `if let Some(engine) = state.pxpipe_engine_for(model)`.
    pub(crate) fn pxpipe_engine_for(
        &self,
        model: &str,
    ) -> Option<Arc<crate::pxpipe::PxpipeEngine>> {
        let engine = self.active_pxpipe()?;
        if crate::pxpipe::model_in_scope(model, &self.pxpipe_models())
            && self.pxpipe_vision_ok(model)
        {
            Some(engine)
        } else {
            None
        }
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
