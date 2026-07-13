//! Opt-in guardrails for model-produced tool calls.
//!
//! This mirrors the narrow tool-call policy behavior from forge-guardrails:
//! advisory nudges are returned to the model as tool results, giving the model
//! one more chance to choose a safer or quieter tool call.

use crate::tools::execution::ToolCall;
use anyllm_translate::anthropic::Tool;
use std::str::FromStr;

mod lsp;
mod quiet;
#[cfg(test)]
mod tests;
mod utils;
mod write_cap;

/// Default maximum string payload size for write/edit policy nudges.
pub const DEFAULT_MAX_WRITE_PAYLOAD_BYTES: usize = 64 * 1024;

/// Process/request-level policy preset for tool-call guardrails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolGuardrailMode {
    /// Do not apply tool-call guardrails.
    Disabled,
    /// Apply all currently supported advisory guardrails.
    Standard,
}

impl ToolGuardrailMode {
    /// Returns the stable string representation of the mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Standard => "standard",
        }
    }
}

impl std::fmt::Display for ToolGuardrailMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ToolGuardrailMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" | "false" | "0" => Ok(Self::Disabled),
            "standard" | "on" | "true" | "1" => Ok(Self::Standard),
            other => Err(format!(
                "tool guardrail mode must be disabled or standard, got '{other}'"
            )),
        }
    }
}

/// Opt-in controls for tool-call advisory guardrails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGuardrailConfig {
    /// Preset used to initialize the individual controls.
    pub mode: ToolGuardrailMode,
    /// Nudge grep/glob/shell-grep symbol searches toward available LSP tools.
    pub lsp_first: bool,
    /// Nudge noisy shell commands toward quieter equivalents.
    pub quiet_commands: bool,
    /// Nudge oversized write/edit payloads.
    pub write_payload_caps: bool,
    /// Maximum payload size before write/edit policy nudges fire.
    pub max_write_payload_bytes: usize,
}

impl Default for ToolGuardrailConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

impl ToolGuardrailConfig {
    /// Returns a disabled configuration.
    pub fn disabled() -> Self {
        Self {
            mode: ToolGuardrailMode::Disabled,
            lsp_first: false,
            quiet_commands: false,
            write_payload_caps: false,
            max_write_payload_bytes: DEFAULT_MAX_WRITE_PAYLOAD_BYTES,
        }
    }

    /// Returns a standard configuration.
    pub fn standard() -> Self {
        Self {
            mode: ToolGuardrailMode::Standard,
            lsp_first: true,
            quiet_commands: true,
            write_payload_caps: true,
            max_write_payload_bytes: DEFAULT_MAX_WRITE_PAYLOAD_BYTES,
        }
    }

    /// Builds a configuration from a preset mode.
    pub fn from_mode(mode: ToolGuardrailMode) -> Self {
        match mode {
            ToolGuardrailMode::Disabled => Self::disabled(),
            ToolGuardrailMode::Standard => Self::standard(),
        }
    }

    /// Returns true when at least one guardrail is enabled.
    pub fn enabled(&self) -> bool {
        self.lsp_first || self.quiet_commands || self.write_payload_caps
    }
}

/// Resolves the guardrail config to use for a single request.
///
/// `static_config` is the per-process preset built from YAML/env at startup
/// (`ToolEngineState.guardrails`). `runtime_mode` is the live, admin-tunable
/// override (`RuntimeConfig.tool_guardrail_mode`, no restart required). When
/// `runtime_mode` parses to a different mode than the static preset, the
/// preset that mode maps to wins for this request; otherwise the static
/// preset is used unchanged (preserving any of its non-mode field overrides,
/// e.g. a custom `max_write_payload_bytes`). An unparseable `runtime_mode`
/// (should not happen once it is only ever admin-written, but defends
/// against a corrupted/legacy DB value) also falls back to the static preset.
pub fn resolve_runtime_guardrails(
    static_config: &ToolGuardrailConfig,
    runtime_mode: &str,
) -> ToolGuardrailConfig {
    match runtime_mode.parse::<ToolGuardrailMode>() {
        Ok(mode) if mode != static_config.mode => ToolGuardrailConfig::from_mode(mode),
        _ => static_config.clone(),
    }
}

/// Read `tool_guardrail_mode` from a live `RuntimeConfig` behind its lock and
/// resolve it against `engine_guardrails`. Single implementation of the
/// "read the lock, extract the mode string, resolve" step so
/// `AppState::effective_tool_guardrails` (which always has a `RuntimeConfig`
/// lock) and the streaming tool loop (which only has one via
/// `Option<SharedState>`, not a full `AppState`) can't drift from each other.
pub fn resolve_runtime_guardrails_locked(
    runtime_config: &std::sync::RwLock<crate::admin::state::RuntimeConfig>,
    engine_guardrails: &ToolGuardrailConfig,
) -> ToolGuardrailConfig {
    let mode = runtime_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .tool_guardrail_mode
        .clone();
    resolve_runtime_guardrails(engine_guardrails, &mode)
}

/// Per-request state used to suppress repeat advisory nudges across tool-loop
/// iterations (not within a single batch — see `evaluate_tool_guardrails`).
#[derive(Debug, Default)]
pub struct ToolGuardrailRequestState {
    seen_fingerprints: indexmap::IndexSet<String>,
}

impl ToolGuardrailRequestState {
    /// Creates empty per-request guardrail state.
    pub fn new() -> Self {
        Self::default()
    }

    /// True if this fingerprint was already nudged in a previous batch
    /// (an earlier tool-loop iteration) of this request.
    fn already_nudged(&self, fingerprint: &str) -> bool {
        self.seen_fingerprints.contains(fingerprint)
    }

    /// Record every fingerprint nudged in the current batch so a later
    /// iteration doesn't nudge the same distinct decision again. Applies to
    /// every guardrail kind so the model is nudged at most once per distinct
    /// decision across iterations; if it ignores the nudge and repeats the
    /// call, the call proceeds normally instead of looping to exhaustion.
    fn mark_nudged_batch(&mut self, fingerprints: indexmap::IndexSet<String>) {
        self.seen_fingerprints.extend(fingerprints);
    }
}

/// Tool-call guardrail nudge returned to the model as a synthetic tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGuardrailNudge {
    /// Id of the specific tool call this nudge targets.
    pub call_id: String,
    /// Stable policy kind for metrics and tests.
    pub kind: &'static str,
    /// Human-readable retry instruction.
    pub content: String,
    /// Stable per-request fingerprint for this decision.
    pub fingerprint: String,
}

/// Evaluates one batch of model-produced tool calls against opt-in guardrails.
///
/// Returns at most one nudge per offending tool call (never for calls that did
/// not trip a guardrail), so legitimate parallel calls in the same batch are
/// left untouched and can still execute. Each distinct decision nudges only
/// once per request (see [`ToolGuardrailRequestState::should_nudge`]).
pub fn evaluate_tool_guardrails(
    tool_calls: &[ToolCall],
    tool_specs: &[Tool],
    config: &ToolGuardrailConfig,
    state: &mut ToolGuardrailRequestState,
) -> Vec<ToolGuardrailNudge> {
    if !config.enabled() {
        return Vec::new();
    }

    let lsp_tools = if config.lsp_first {
        lsp::available_lsp_tools(tool_specs)
    } else {
        Vec::new()
    };

    let mut nudges = Vec::new();
    let mut nudged_fingerprints_this_batch: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    for call in tool_calls {
        let candidate = (!lsp_tools.is_empty())
            .then(|| lsp::lsp_first_nudge(call, &lsp_tools))
            .flatten()
            .or_else(|| {
                config
                    .write_payload_caps
                    .then(|| {
                        write_cap::write_payload_cap_nudge(call, config.max_write_payload_bytes)
                    })
                    .flatten()
            })
            .or_else(|| {
                config
                    .quiet_commands
                    .then(|| quiet::quiet_command_nudge(call))
                    .flatten()
            });

        if let Some(nudge) = candidate {
            // Nudge every call in this batch matching a fingerprint not
            // already nudged in a prior iteration — two identical parallel
            // calls in the same turn must be treated consistently, not have
            // only the first one flagged and the second execute unchecked.
            if !state.already_nudged(&nudge.fingerprint) {
                nudged_fingerprints_this_batch.insert(nudge.fingerprint.clone());
                nudges.push(nudge);
            }
        }
    }
    state.mark_nudged_batch(nudged_fingerprints_this_batch);

    nudges
}
