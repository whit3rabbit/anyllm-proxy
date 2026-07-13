//! Opt-in prompt compression (Frozen-Frontier Extractive Compression / FFEC).
//!
//! Mirrors `rtk.rs` and forge-guardrails: an IO-free algorithm crate
//! (`anyllm_optimize_core` + `anyllm_optimize_passes`) is wrapped here in a thin,
//! fail-open, transform-only shim. Gating (mode + route scope) lives on `AppState`
//! so it reads the live `RuntimeConfig`; see `crates/optimizer/CLAUDE.md`
//! "Proxy integration checklist" for the full wiring plan this module is step 2 of.
//!
//! **Opt-in cascade** mirrors rtk/pxpipe: `OPTIMIZER_MODE=off|shadow|live` env seeds
//! `RuntimeConfig.optimizer_mode` (live admin toggle, applied by
//! `AppState::effective_optimizer` between the route and static tiers).
//! `Shadow` runs the full pipeline and reports would-be savings but never mutates the
//! body; `Live` renders the compressed body back in place. `Off` (the default) skips
//! the pipeline entirely.
//!
//! Determinism keeps prompt caches stable: `optimize()` is a pure function of message
//! bytes + policy version (see `crates/optimizer/CLAUDE.md` invariants). This shim adds
//! one more fail-open layer on top of `optimize()`'s own `catch_unwind`: the adapter's
//! `from_value`/`apply_rendered` JSON walk is also wrapped, so a bug in the adapter
//! layer can never propagate a panic into the request path.

use anyllm_optimize_core::{
    HeuristicBudgetCounter, Mode, OptimizationPolicy, OptimizationReport, TokenScorer,
    UniformScorer, Workspace,
};
use anyllm_optimize_passes::adapter::{anthropic, openai};
use anyllm_optimize_passes::{AnthropicStrategy, OpenAiStrategy};
use anyllm_optimize_scorer::artifact::ArtifactConfig;
use bytes::Bytes;
use serde_json::Value;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, RwLock};

/// Whether this proxy binary was built with the ONNX scorer (`optimizer-onnx` feature).
/// When false, the model download/load UI is inert and live-mode uses the heuristic
/// `UniformScorer`. Surfaced to the admin UI via [`OptimizerEngine::model_status`].
pub const ONNX_COMPILED_IN: bool = cfg!(feature = "optimizer-onnx");

/// Transform-only FFEC engine. Held on `AppState`; gating is external.
pub struct OptimizerEngine {
    policy: OptimizationPolicy,
    /// Lazily-loaded ONNX scorer. `None` => heuristic `UniformScorer`. `Arc`-shared so
    /// per-request `with_mode_override` clones share one loaded model, and so a scorer
    /// loaded on the first live request after a download is seen by every later request.
    scorer: Arc<RwLock<Option<Arc<dyn TokenScorer>>>>,
    /// Resolved model artifact pin + cache location. `None` when no model tier is
    /// configured (e.g. `new` for tests). `Arc`-shared across clones.
    model: Option<Arc<ArtifactConfig>>,
}

impl OptimizerEngine {
    /// Build an engine whose top-level mode is `mode`, with no ONNX model tier
    /// (heuristic `UniformScorer` only). Used by tests and heuristic-only setups.
    pub fn new(mode: Mode) -> Self {
        Self::with_model(mode, None)
    }

    /// Build an engine with an optional ONNX model artifact config. If `model` is set and
    /// the artifact is already present on disk, the scorer is loaded eagerly so live-mode
    /// uses it from the first request (no restart needed after a prior download).
    pub fn with_model(mode: Mode, model: Option<ArtifactConfig>) -> Self {
        let engine = Self {
            policy: OptimizationPolicy {
                mode,
                ..OptimizationPolicy::default()
            },
            scorer: Arc::new(RwLock::new(None)),
            model: model.map(Arc::new),
        };
        engine.ensure_scorer_loaded();
        engine
    }

    /// Resolved model artifact config, if a model tier is configured.
    pub fn model_config(&self) -> Option<&ArtifactConfig> {
        self.model.as_deref()
    }

    /// Load the ONNX scorer from the configured, present artifact into the shared slot.
    /// No-op (returns `false`) without the `optimizer-onnx` feature, when no model tier is
    /// configured, when the artifact isn't present, or when it's already loaded. Fail-open:
    /// a load error is logged and leaves the slot empty (heuristic scorer stays in use).
    pub fn ensure_scorer_loaded(&self) -> bool {
        #[cfg(feature = "optimizer-onnx")]
        {
            if self
                .scorer
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
            {
                return true;
            }
            let Some(cfg) = self.model.as_ref() else {
                return false;
            };
            if !cfg.is_present() {
                return false;
            }
            match anyllm_optimize_scorer::LLMLingua2Pass::from_files(
                cfg.onnx_path(),
                cfg.tokenizer_path(),
                cfg.artifact_hash(),
            ) {
                Ok(pass) => {
                    *self.scorer.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(pass));
                    tracing::info!("optimizer: loaded LLMLingua-2 ONNX scorer");
                    true
                }
                Err(e) => {
                    tracing::warn!(error = %e, "optimizer: failed to load ONNX scorer; using heuristic");
                    false
                }
            }
        }
        #[cfg(not(feature = "optimizer-onnx"))]
        {
            false
        }
    }

    /// Compress an OpenAI Chat Completions body (`root["messages"]`) in place for
    /// `route`. Fails open (leaves `root` unchanged) on any panic in the adapter/
    /// algorithm pipeline. `Shadow` mode never mutates `root` even on success — the
    /// report still carries the would-be savings for observability.
    pub fn optimize_openai(&self, root: &mut Value, route: &str) -> OptimizationReport {
        self.run(
            root,
            route,
            openai::from_value,
            openai::apply_rendered,
            OpenAiStrategy::default,
        )
    }

    /// Compress an Anthropic Messages body (`root["messages"]`) in place for `route`.
    /// `root["system"]` is never touched (Immutable, not part of the IR). Same
    /// fail-open contract as [`Self::optimize_openai`].
    pub fn optimize_anthropic(&self, root: &mut Value, route: &str) -> OptimizationReport {
        self.run(
            root,
            route,
            anthropic::from_value,
            anthropic::apply_rendered,
            AnthropicStrategy::default,
        )
    }

    /// This engine's top-level policy mode. Lets a caller skip the (up to 32MB)
    /// JSON parse in [`Self::optimize_anthropic_bytes`] when the resolved mode is
    /// `Off`; route-level overrides are not populated yet, so this equals the
    /// effective mode for every route today.
    pub fn mode(&self) -> Mode {
        self.policy.mode
    }

    /// Compress an Anthropic Messages body given as raw bytes, for the Anthropic
    /// passthrough path (`BACKEND=anthropic`). Unlike
    /// [`Self::apply_optimizer_to_anthropic`]'s typed seam, this never round-trips
    /// through `MessageCreateRequest`, so the frontier `cache_control` breakpoint
    /// (and any block/tool type the struct doesn't model) survives to the wire.
    /// Mirrors `RtkEngine::compress_anthropic`: fails open (returns `body`
    /// unchanged) on any parse/serialize error, in `Shadow` mode, or when nothing
    /// compressed.
    pub fn optimize_anthropic_bytes(
        &self,
        body: Bytes,
        route: &str,
        metrics: &crate::metrics::Metrics,
    ) -> Bytes {
        let mut root: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return body,
        };
        let report = self.optimize_anthropic(&mut root, route);
        if report.mode == Mode::Shadow {
            tracing::info!(
                route,
                removed_tokens_est = report.removed_tokens_est,
                messages_compressed = report.messages_compressed,
                failure = report.failure.as_deref().unwrap_or(""),
                "optimizer: shadow report (not applied)"
            );
        }
        if !report.applied {
            return body;
        }
        match serde_json::to_vec(&root) {
            Ok(bytes) => {
                // ponytail: length guard mirrors rtk::compress_anthropic. Can skip a
                // pure cache-marker placement that didn't shrink bytes; acceptable
                // because Live-mode frontier compression removes far more text than
                // the marker adds. Upgrade path: compare token estimate, not bytes,
                // if a cache-only optimization ever matters more than raw size.
                if bytes.len() >= body.len() {
                    return body;
                }
                metrics.record_optimization(
                    report.messages_compressed as u64,
                    report.removed_tokens_est,
                );
                tracing::info!(
                    route,
                    removed_tokens_est = report.removed_tokens_est,
                    messages_compressed = report.messages_compressed,
                    bytes_before = body.len(),
                    bytes_after = bytes.len(),
                    "optimizer: compressed Anthropic request"
                );
                Bytes::from(bytes)
            }
            Err(_) => body,
        }
    }

    /// Return a new engine with `mode_str` (parsed via `Mode::from_str`,
    /// `off|shadow|live`) substituted for the top-level policy mode, keeping
    /// everything else (route overrides, ratios, frontier) from `self`. An
    /// unparseable override falls back to `self`'s own policy unchanged --
    /// fail-safe, mirrors `tools::resolve_runtime_guardrails`'s "unparseable
    /// falls back to static" contract. Used by `AppState::effective_optimizer`
    /// to apply a live route-level mode override without rebuilding the whole
    /// policy from scratch.
    pub fn with_mode_override(&self, mode_str: &str) -> Self {
        let mode = mode_str.parse::<Mode>().unwrap_or(self.policy.mode);
        Self {
            policy: OptimizationPolicy {
                mode,
                ..self.policy.clone()
            },
            // Share the loaded scorer / model slots so a per-request clone never triggers
            // a reload and sees a scorer another request loaded.
            scorer: self.scorer.clone(),
            model: self.model.clone(),
        }
    }

    fn run<S: anyllm_optimize_core::CacheStrategy>(
        &self,
        root: &mut Value,
        route: &str,
        from_value: fn(&Value) -> anyllm_optimize_core::Conversation,
        apply_rendered: fn(&mut Value, &anyllm_optimize_core::RenderedConversation),
        strategy: impl FnOnce() -> S,
    ) -> OptimizationReport {
        let policy = self.policy.resolve(route);
        if policy.mode == Mode::Off {
            return OptimizationReport::failed_open(Mode::Off, policy.compression.version, "off");
        }
        let version = policy.compression.version;

        // Belt-and-braces fail-open: `optimize()` already catches panics inside its own
        // pipeline, but the adapter JSON walk (from_value / apply_rendered) runs outside
        // that boundary, so wrap the whole thing too.
        let strategy = strategy();
        // Lazy-load: if a model was downloaded after startup (e.g. via the admin button on
        // this or the sibling admin server), pick it up on the next non-Off request without
        // a restart. Cheap no-op once loaded or when absent (a sidecar stat).
        self.ensure_scorer_loaded();
        // Use the loaded ONNX scorer if present, else the heuristic UniformScorer. The
        // read guard is held across optimize() — writes (a fresh download load) are rare.
        let scorer_guard = self.scorer.read().unwrap_or_else(|e| e.into_inner());
        let fallback = UniformScorer;
        let scorer: &dyn TokenScorer = match scorer_guard.as_ref() {
            Some(loaded) => loaded.as_ref(),
            None => &fallback,
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let conv = from_value(root);
            let mut ws = Workspace::new();
            anyllm_optimize_core::optimize(
                &conv,
                &policy,
                &strategy,
                scorer,
                &HeuristicBudgetCounter::default(),
                &mut ws,
            )
        }));

        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) => {
                return OptimizationReport::failed_open(
                    policy.mode,
                    version,
                    "panic in optimizer adapter",
                );
            }
        };

        if let Some(rendered) = &outcome.rendered {
            let applied = std::panic::catch_unwind(AssertUnwindSafe(|| {
                apply_rendered(root, rendered);
            }));
            if applied.is_err() {
                return OptimizationReport::failed_open(
                    policy.mode,
                    version,
                    "panic applying optimizer render",
                );
            }
        }
        outcome.report
    }
}

/// Transient state of the (admin-triggered) model download. Process-global (one model,
/// one download at a time) so the admin server can drive it without reaching into the
/// proxy's `OptimizerEngine`; the proxy picks up the downloaded artifact via the engine's
/// lazy load on the next request. Mirrors the `virtual_keys` global-state pattern.
#[derive(Default, Clone)]
pub struct DownloadState {
    /// A download+verify is in flight.
    pub running: bool,
    /// Last download error (cleared when a new download starts).
    pub error: Option<String>,
}

static MODEL_DOWNLOAD: std::sync::OnceLock<Mutex<DownloadState>> = std::sync::OnceLock::new();

fn download_state() -> &'static Mutex<DownloadState> {
    MODEL_DOWNLOAD.get_or_init(|| Mutex::new(DownloadState::default()))
}

/// JSON-serializable snapshot of the ONNX model tier for `GET /admin/api/optimizer/model`.
#[derive(serde::Serialize)]
pub struct ModelStatus {
    /// Proxy built with `optimizer-onnx`. When false the download/enable UI is inert.
    pub compiled_in: bool,
    /// Verified artifact is on disk (cheap sidecar check).
    pub present: bool,
    /// A download+verify is in flight.
    pub downloading: bool,
    /// Last download error, if any.
    pub error: Option<String>,
    /// Pinned sha256 the download is verified against (empty if pin unresolved).
    pub sha256: String,
    /// Expected download size in bytes (for a UI progress hint).
    pub size_bytes: u64,
}

/// Current model-tier status for the admin UI. Resolves the pin + cache dir the same way
/// the proxy engine does, so `present` reflects exactly what the engine would load.
pub fn model_status() -> ModelStatus {
    let cfg = resolve_model_config();
    let dl = download_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    ModelStatus {
        compiled_in: ONNX_COMPILED_IN,
        present: cfg.as_ref().is_some_and(|c| c.is_present()),
        downloading: dl.running,
        error: dl.error,
        sha256: cfg.as_ref().map(|c| c.sha256.clone()).unwrap_or_default(),
        size_bytes: anyllm_optimize_scorer::artifact::MODEL_ONNX_BYTES,
    }
}

/// Claim the download slot. Returns `Err` if a download is already running or the pin is
/// unresolved. On `Ok`, the caller MUST eventually call [`run_model_download_blocking`].
pub fn begin_model_download() -> Result<ArtifactConfig, String> {
    let cfg = resolve_model_config().ok_or("model artifact pin unresolved (bad MODEL_SHA256?)")?;
    let mut dl = download_state().lock().unwrap_or_else(|e| e.into_inner());
    if dl.running {
        return Err("a model download is already in progress".to_string());
    }
    dl.running = true;
    dl.error = None;
    Ok(cfg)
}

/// Run the blocking download+verify (call inside `spawn_blocking`), then clear the running
/// flag and record any error. The proxy engine loads the scorer lazily on its next request.
pub fn run_model_download_blocking(cfg: &ArtifactConfig) {
    let result = anyllm_optimize_scorer::artifact::download_and_verify(cfg, false)
        .map(|_| ())
        .map_err(|e| e.to_string());
    let mut dl = download_state().lock().unwrap_or_else(|e| e.into_inner());
    dl.running = false;
    dl.error = result.err();
}

/// Resolve the ONNX model artifact config: pinned URL + sha256 (overridable via
/// `MODEL_URL`/`MODEL_SHA256`), cached under `<data_dir>/models` (overridable via
/// `MODEL_CACHE_DIR`). Returns `None` only if the pin resolution fails (bad
/// `MODEL_SHA256`), in which case the ONNX tier stays unavailable (heuristic only).
pub fn resolve_model_config() -> Option<ArtifactConfig> {
    let default_cache = crate::config::helpers::resolve_data_dir().join("models");
    match ArtifactConfig::resolve(default_cache) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            tracing::warn!(error = %e, "optimizer: could not resolve model artifact config");
            None
        }
    }
}

/// Resolve the default mode from `OPTIMIZER_MODE` (`off|shadow|live`, parsed via
/// `Mode::from_str`). Seeds `RuntimeConfig.optimizer_mode`. Defaults to `Off`
/// (opt-in), matching `rtk::resolve_default_enabled` / pxpipe's opt-in cascade.
pub fn resolve_default_mode() -> Mode {
    std::env::var("OPTIMIZER_MODE")
        .ok()
        .and_then(|v| v.parse::<Mode>().ok())
        .unwrap_or(Mode::Off)
}

/// Apply the live `RuntimeConfig.optimizer_mode` override on top of `engine`'s
/// static (`OPTIMIZER_MODE`-env) policy. Mirrors
/// `tools::resolve_runtime_guardrails`'s contract: an unparseable runtime value
/// falls back to the engine's own mode unchanged (fail-safe), and this is cheap
/// enough to call unconditionally (unlike guardrails' preset rebuild, rebuilding
/// `OptimizationPolicy` with a new `mode` field is just a struct-update clone).
pub fn resolve_runtime_optimizer(engine: &OptimizerEngine, runtime_mode: &str) -> OptimizerEngine {
    engine.with_mode_override(runtime_mode)
}

/// Read `optimizer_mode` from a live `RuntimeConfig` behind its lock and
/// resolve it against `engine`. Single implementation of the "read the lock,
/// extract the mode string, resolve" step so `AppState::effective_optimizer`
/// can't drift from any other call site that needs the same tier, mirroring
/// `tools::resolve_runtime_guardrails_locked`.
pub fn resolve_runtime_optimizer_locked(
    runtime_config: &std::sync::RwLock<crate::admin::state::RuntimeConfig>,
    engine: &OptimizerEngine,
) -> OptimizerEngine {
    let mode = runtime_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .optimizer_mode
        .clone();
    resolve_runtime_optimizer(engine, &mode)
}

#[cfg(test)]
#[path = "optimizer/tests.rs"]
mod tests;
