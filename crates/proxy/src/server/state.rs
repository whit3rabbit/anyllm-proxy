// Shared state types for request handlers: AppState, AnthropicJson, ResolvedModel, etc.
// Extracted from routes.rs so consumers can import state independently of the router setup.

use crate::admin::state::{RuntimeConfig, SharedState};
use crate::backend::BackendClient;
use crate::metrics::Metrics;
use anyllm_providers::ProviderCatalog;
use anyllm_translate::{anthropic, mapping, openai};
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
    /// Command-aware tool-output compression engine (RTK). `Some` for Anthropic
    /// and Translate modes; only consulted when `RuntimeConfig.rtk_compress` is
    /// on -- use `rtk_engine_for(model)`.
    pub rtk: Option<Arc<crate::rtk::RtkEngine>>,
    /// FFEC prompt-compression engine (`OptimizerEngine`). `Some` for Anthropic
    /// and Translate modes, mirroring `rtk`; baked with the static
    /// `OPTIMIZER_MODE`-env default at startup -- use `effective_optimizer()`,
    /// which applies the live `RouteOptions.optimizer_mode` override on top.
    pub optimizer: Option<Arc<crate::optimizer::OptimizerEngine>>,
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

    /// The RTK engine for `model`, or `None` if compression shouldn't run: the
    /// toggle is off, the engine is absent, or the model is out of scope. RTK is
    /// not vision-gated, so there is no capability check.
    ///
    /// Reads the toggle and scope from a single RwLock critical section for
    /// consistency, and checks `route_options` first (matching the pxpipe pattern)
    /// so per-route overrides take precedence over the global RuntimeConfig.
    pub(crate) fn rtk_engine_for(&self, model: &str) -> Option<Arc<crate::rtk::RtkEngine>> {
        let cfg = self
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let enabled = self
            .route_options
            .as_ref()
            .and_then(|o| o.rtk_compress)
            .unwrap_or(cfg.rtk_compress);
        if !enabled {
            return None;
        }
        let engine = self.rtk.clone()?;
        let models_csv = self
            .route_options
            .as_ref()
            .and_then(|o| o.rtk_models.as_deref())
            .unwrap_or(&cfg.rtk_models);
        if crate::rtk::model_in_scope(model, models_csv) {
            Some(engine)
        } else {
            None
        }
    }

    /// Effective FFEC prompt-compression engine for this request, or `None`
    /// when optimization is unconfigured for this backend/mode (`self.optimizer`
    /// is `None`). Precedence, mirroring `effective_tool_guardrails` /
    /// `resolve_runtime_guardrails_locked`: (1) route override
    /// (`RouteOptions.optimizer_mode`, if set) wins outright; (2) otherwise the
    /// live `RuntimeConfig.optimizer_mode` admin toggle (no restart required);
    /// (3) otherwise the static per-process engine baked with the
    /// `OPTIMIZER_MODE`-env default at startup.
    pub(crate) fn effective_optimizer(&self) -> Option<Arc<crate::optimizer::OptimizerEngine>> {
        let engine = self.optimizer.as_ref()?;
        if let Some(mode_str) = self
            .route_options
            .as_ref()
            .and_then(|o| o.optimizer_mode.as_deref())
        {
            return Some(Arc::new(engine.with_mode_override(mode_str)));
        }
        Some(Arc::new(
            crate::optimizer::resolve_runtime_optimizer_locked(&self.runtime_config, engine),
        ))
    }

    /// Apply RTK tool-output compression to an OpenAI-format request and record
    /// metrics. Shared helper used by both the /v1/chat/completions and /v1/messages
    /// translate paths (streaming and non-streaming). No-op when the engine is
    /// unavailable, disabled, or no tool messages are present.
    pub(crate) fn apply_rtk_to_openai(&self, req: &mut openai::ChatCompletionRequest, model: &str) {
        let engine = match self.rtk_engine_for(model) {
            Some(e) => e,
            None => return,
        };
        // Pre-check: only serialize when there are tool messages to compress.
        if !req
            .messages
            .iter()
            .any(|m| m.role == openai::ChatRole::Tool)
        {
            return;
        }
        let mut v = match serde_json::to_value(&*req) {
            Ok(v) => v,
            Err(_) => return,
        };
        let Some((blocks, saved)) = engine.compress_openai_chat(&mut v) else {
            return;
        };
        match serde_json::from_value::<openai::ChatCompletionRequest>(v) {
            Ok(patched) => {
                *req = patched;
                self.metrics.record_rtk_compression(blocks, saved);
                tracing::info!(
                    model,
                    blocks,
                    chars_saved = saved,
                    "rtk: compressed OpenAI request"
                );
            }
            Err(e) => tracing::warn!(
                error = %e,
                "rtk: failed to re-deserialize compressed OpenAI request; forwarding original"
            ),
        }
    }

    /// Apply FFEC prompt compression (`effective_optimizer()`) to an OpenAI-format
    /// request at the parsed-body seam. Client-sent history only -- callers must
    /// never invoke this on proxy-appended tool-loop turns (see
    /// `crates/optimizer/CLAUDE.md` "Streaming & tool-loop decision"). `Shadow`
    /// mode logs the `OptimizationReport` and leaves `req` unchanged; `Live` mode
    /// applies the rendered body in place. No-op when optimization is
    /// unconfigured or resolves to `Mode::Off` for this request.
    pub(crate) fn apply_optimizer_to_openai(
        &self,
        req: &mut openai::ChatCompletionRequest,
        route: &str,
    ) {
        let Some(engine) = self.effective_optimizer() else {
            return;
        };
        let mut v = match serde_json::to_value(&*req) {
            Ok(v) => v,
            Err(_) => return,
        };
        let report = engine.optimize_openai(&mut v, route);
        if report.mode == anyllm_optimize_core::Mode::Shadow {
            tracing::info!(
                route,
                removed_tokens_est = report.removed_tokens_est,
                messages_compressed = report.messages_compressed,
                failure = report.failure.as_deref().unwrap_or(""),
                "optimizer: shadow report (not applied)"
            );
        }
        if !report.applied {
            return;
        }
        match serde_json::from_value::<openai::ChatCompletionRequest>(v) {
            Ok(patched) => {
                *req = patched;
                self.metrics.record_optimization(
                    report.messages_compressed as u64,
                    report.removed_tokens_est,
                );
                tracing::info!(
                    route,
                    removed_tokens_est = report.removed_tokens_est,
                    messages_compressed = report.messages_compressed,
                    "optimizer: compressed OpenAI request"
                );
            }
            Err(e) => tracing::warn!(
                error = %e,
                "optimizer: failed to re-deserialize compressed OpenAI request; forwarding original"
            ),
        }
    }

    /// Apply FFEC prompt compression (`effective_optimizer()`) to an Anthropic
    /// Messages request at the parsed-body seam. Same contract as
    /// [`Self::apply_optimizer_to_openai`]: client-sent history only, fails open,
    /// `Shadow` never mutates `req`.
    pub(crate) fn apply_optimizer_to_anthropic(
        &self,
        req: &mut anthropic::MessageCreateRequest,
        route: &str,
    ) {
        let Some(engine) = self.effective_optimizer() else {
            return;
        };
        let mut v = match serde_json::to_value(&*req) {
            Ok(v) => v,
            Err(_) => return,
        };
        let report = engine.optimize_anthropic(&mut v, route);
        if report.mode == anyllm_optimize_core::Mode::Shadow {
            tracing::info!(
                route,
                removed_tokens_est = report.removed_tokens_est,
                messages_compressed = report.messages_compressed,
                failure = report.failure.as_deref().unwrap_or(""),
                "optimizer: shadow report (not applied)"
            );
        }
        if !report.applied {
            return;
        }
        match serde_json::from_value::<anthropic::MessageCreateRequest>(v) {
            Ok(patched) => {
                *req = patched;
                self.metrics.record_optimization(
                    report.messages_compressed as u64,
                    report.removed_tokens_est,
                );
                tracing::info!(
                    route,
                    removed_tokens_est = report.removed_tokens_est,
                    messages_compressed = report.messages_compressed,
                    "optimizer: compressed Anthropic request"
                );
            }
            Err(e) => tracing::warn!(
                error = %e,
                "optimizer: failed to re-deserialize compressed Anthropic request; forwarding original"
            ),
        }
    }
}

#[cfg(test)]
mod optimizer_seam_tests {
    use super::*;
    use crate::config::{
        BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat, TlsConfig,
    };
    use anyllm_optimize_core::Mode;

    /// Long enough that FFEC's min-length gate actually has something to compress.
    fn long_text() -> String {
        "The quick brown fox jumps over the lazy dog again and again across the wide \
         green field toward the distant blue mountains far beyond the winding river."
            .repeat(4)
    }

    fn minimal_state(optimizer_mode: Mode) -> AppState {
        let config = Config {
            backend: BackendKind::OpenAI,
            openai_api_key: "test".into(),
            openai_base_url: "https://api.openai.com".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gpt-4o".into(),
                small_model: "gpt-4o-mini".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::BearerToken("test".into()),
            log_bodies: false,
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
            provider_id: None,
        };
        let backend = crate::backend::BackendClient::OpenAI(
            crate::backend::openai_client::OpenAIClient::new(&config),
        );
        let runtime_config = Arc::new(RwLock::new(RuntimeConfig {
            model_mappings: indexmap::IndexMap::new(),
            log_level: "info".to_string(),
            log_bodies: false,
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            pxpipe_models: String::new(),
            rtk_compress: false,
            rtk_models: String::new(),
            forward_client_auth: false,
            tool_guardrail_mode: "disabled".to_string(),
            optimizer_mode: optimizer_mode.as_str().to_string(),
        }));
        AppState {
            backend,
            metrics: Metrics::new(),
            runtime_config,
            shared: None,
            route_options: None,
            backend_name: "openai".to_string(),
            provider_id: None,
            concurrency: Arc::new(Semaphore::new(64)),
            omit_stream_options: false,
            stream_timeout_secs: 0,
            expose_degradation_warnings: false,
            cache: None,
            thinking_repair: None,
            pxpipe: None,
            rtk: None,
            optimizer: Some(Arc::new(crate::optimizer::OptimizerEngine::new(
                optimizer_mode,
            ))),
            model_router: None,
            provider_catalog: Arc::new(ProviderCatalog::bundled()),
            all_backends: None,
            tool_engine: None,
            batch_engine: None,
        }
    }

    fn long_openai_request() -> openai::ChatCompletionRequest {
        let long = long_text();
        let mut body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "system", "content": "you are helpful"}],
        });
        let msgs = body["messages"].as_array_mut().unwrap();
        for _ in 0..16 {
            msgs.push(serde_json::json!({"role": "user", "content": long}));
            msgs.push(serde_json::json!({"role": "assistant", "content": long}));
        }
        msgs.push(serde_json::json!({"role": "user", "content": "what is the latest?"}));
        serde_json::from_value(body).expect("valid ChatCompletionRequest")
    }

    fn long_anthropic_request() -> anthropic::MessageCreateRequest {
        let long = long_text();
        let mut body = serde_json::json!({
            "model": "claude-sonnet-5",
            "max_tokens": 1024,
            "messages": [],
        });
        let msgs = body["messages"].as_array_mut().unwrap();
        for _ in 0..16 {
            msgs.push(serde_json::json!({"role": "user", "content": long}));
            msgs.push(serde_json::json!({"role": "assistant", "content": long}));
        }
        msgs.push(serde_json::json!({"role": "user", "content": "what is the latest?"}));
        serde_json::from_value(body).expect("valid MessageCreateRequest")
    }

    #[test]
    fn shadow_mode_forwards_openai_body_unchanged() {
        let state = minimal_state(Mode::Shadow);
        let mut req = long_openai_request();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_openai(&mut req, "chat_completions");
        let after = serde_json::to_value(&req).unwrap();
        assert_eq!(before, after, "shadow mode must forward the original body");
        assert_eq!(
            state.metrics.snapshot().optimizer_compressed_total,
            0,
            "shadow mode must never record a metrics-visible compression"
        );
    }

    #[test]
    fn shadow_mode_forwards_anthropic_body_unchanged() {
        let state = minimal_state(Mode::Shadow);
        let mut req = long_anthropic_request();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_anthropic(&mut req, "messages");
        let after = serde_json::to_value(&req).unwrap();
        assert_eq!(before, after, "shadow mode must forward the original body");
    }

    #[test]
    fn live_mode_compresses_openai_history_and_preserves_latest() {
        let state = minimal_state(Mode::Live);
        let mut req = long_openai_request();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_openai(&mut req, "chat_completions");
        let after = serde_json::to_value(&req).unwrap();
        assert_eq!(
            before["messages"].as_array().unwrap().last(),
            after["messages"].as_array().unwrap().last(),
            "the latest turn must never be rewritten"
        );
        assert_eq!(
            state.metrics.snapshot().optimizer_compressed_total,
            1,
            "an applied Live compression must be recorded in metrics"
        );
    }

    #[test]
    fn live_mode_compresses_anthropic_history_and_preserves_latest() {
        let state = minimal_state(Mode::Live);
        let mut req = long_anthropic_request();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_anthropic(&mut req, "messages");
        let after = serde_json::to_value(&req).unwrap();
        assert_eq!(
            before["messages"].as_array().unwrap().last(),
            after["messages"].as_array().unwrap().last(),
            "the latest turn must never be rewritten"
        );
    }

    #[test]
    fn off_mode_is_noop_and_engine_absent_is_noop() {
        // Off mode: engine present, mode Off -> never applied.
        let state = minimal_state(Mode::Off);
        let mut req = long_openai_request();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_openai(&mut req, "chat_completions");
        assert_eq!(before, serde_json::to_value(&req).unwrap());

        // No engine at all (e.g. non-Anthropic/Translate mode backend): no panic, no-op.
        let mut state_no_engine = minimal_state(Mode::Live);
        state_no_engine.optimizer = None;
        let mut req2 = long_openai_request();
        let before2 = serde_json::to_value(&req2).unwrap();
        state_no_engine.apply_optimizer_to_openai(&mut req2, "chat_completions");
        assert_eq!(before2, serde_json::to_value(&req2).unwrap());
    }

    #[test]
    fn short_history_below_min_len_gate_is_a_noop_not_a_panic() {
        // A short request has nothing worth compressing (below FFEC's min-length
        // gate) -- the seam must still round-trip cleanly without panicking or
        // corrupting the body, i.e. it fails open when there's nothing to do.
        let state = minimal_state(Mode::Live);
        let mut req: openai::ChatCompletionRequest = serde_json::from_value(serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .unwrap();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_optimizer_to_openai(&mut req, "chat_completions");
        assert_eq!(before, serde_json::to_value(&req).unwrap());
    }
}

#[cfg(test)]
mod rtk_seam_tests {
    use super::*;

    /// Build a minimal `AppState` with the RTK engine present and the runtime
    /// `rtk_compress` toggle set to `enabled` (scope left empty = all models).
    fn state_with_rtk(enabled: bool) -> AppState {
        use crate::config::{
            BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat, TlsConfig,
        };
        let config = Config {
            backend: BackendKind::OpenAI,
            openai_api_key: "test".into(),
            openai_base_url: "https://api.openai.com".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gpt-4o".into(),
                small_model: "gpt-4o-mini".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::BearerToken("test".into()),
            log_bodies: false,
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
            provider_id: None,
        };
        let backend = crate::backend::BackendClient::OpenAI(
            crate::backend::openai_client::OpenAIClient::new(&config),
        );
        let runtime_config = Arc::new(RwLock::new(RuntimeConfig {
            model_mappings: indexmap::IndexMap::new(),
            log_level: "info".to_string(),
            log_bodies: false,
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            pxpipe_models: String::new(),
            rtk_compress: enabled,
            rtk_models: String::new(),
            forward_client_auth: false,
            tool_guardrail_mode: "disabled".to_string(),
            optimizer_mode: "off".to_string(),
        }));
        AppState {
            backend,
            metrics: Metrics::new(),
            runtime_config,
            shared: None,
            route_options: None,
            backend_name: "openai".to_string(),
            provider_id: None,
            concurrency: Arc::new(Semaphore::new(64)),
            omit_stream_options: false,
            stream_timeout_secs: 0,
            expose_degradation_warnings: false,
            cache: None,
            thinking_repair: None,
            pxpipe: None,
            rtk: Some(Arc::new(crate::rtk::RtkEngine::new())),
            optimizer: None,
            model_router: None,
            provider_catalog: Arc::new(ProviderCatalog::bundled()),
            all_backends: None,
            tool_engine: None,
            batch_engine: None,
        }
    }

    /// An OpenAI request whose `role: tool` message carries compressible git noise.
    fn request_with_noisy_tool_output() -> openai::ChatCompletionRequest {
        let mut noise = String::from("On branch main\nChanges not staged for commit:\n");
        for i in 0..200 {
            noise.push_str(&format!("  (use \"git add ...\" file {i})\n"));
        }
        serde_json::from_value(serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "run git status"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "t1", "type": "function",
                     "function": {"name": "bash", "arguments": "{\"cmd\":\"git status\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "t1", "content": noise},
            ],
        }))
        .expect("valid ChatCompletionRequest")
    }

    #[test]
    fn enabled_compresses_tool_output_and_records_metrics() {
        let state = state_with_rtk(true);
        let mut req = request_with_noisy_tool_output();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_rtk_to_openai(&mut req, "gpt-4o");
        let after = serde_json::to_value(&req).unwrap();
        assert_ne!(before, after, "tool output should have been compressed");
        assert_eq!(state.metrics.snapshot().rtk_compressed_total, 1);
    }

    #[test]
    fn disabled_is_a_noop() {
        let state = state_with_rtk(false);
        let mut req = request_with_noisy_tool_output();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_rtk_to_openai(&mut req, "gpt-4o");
        assert_eq!(before, serde_json::to_value(&req).unwrap());
        assert_eq!(state.metrics.snapshot().rtk_compressed_total, 0);
    }

    #[test]
    fn engine_absent_is_a_noop() {
        let mut state = state_with_rtk(true);
        state.rtk = None;
        let mut req = request_with_noisy_tool_output();
        let before = serde_json::to_value(&req).unwrap();
        state.apply_rtk_to_openai(&mut req, "gpt-4o");
        assert_eq!(before, serde_json::to_value(&req).unwrap());
        assert_eq!(state.metrics.snapshot().rtk_compressed_total, 0);
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
