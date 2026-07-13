use anyllm_translate::{anthropic, openai};
use std::sync::Arc;

use super::app_state::AppState;

impl AppState {
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
#[path = "compression/tests.rs"]
mod tests;
