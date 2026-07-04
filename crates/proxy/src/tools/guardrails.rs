//! Opt-in guardrails for model-produced tool calls.
//!
//! This mirrors the narrow tool-call policy behavior from forge-guardrails:
//! advisory nudges are returned to the model as tool results, giving the model
//! one more chance to choose a safer or quieter tool call.

use crate::tools::execution::ToolCall;
use anyllm_translate::anthropic::Tool;
use serde_json::{Map, Value};
use std::str::FromStr;

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
        available_lsp_tools(tool_specs)
    } else {
        Vec::new()
    };

    let mut nudges = Vec::new();
    let mut nudged_fingerprints_this_batch: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    for call in tool_calls {
        let candidate = (!lsp_tools.is_empty())
            .then(|| lsp_first_nudge(call, &lsp_tools))
            .flatten()
            .or_else(|| {
                config
                    .write_payload_caps
                    .then(|| write_payload_cap_nudge(call, config.max_write_payload_bytes))
                    .flatten()
            })
            .or_else(|| {
                config
                    .quiet_commands
                    .then(|| quiet_command_nudge(call))
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

fn available_lsp_tools(tool_specs: &[Tool]) -> Vec<String> {
    let supported = [
        "find_definition",
        "find_references",
        "get_hover",
        "document_symbols",
        "workspace_symbols",
    ];
    tool_specs
        .iter()
        .filter(|tool| supported.contains(&tool.name.as_str()))
        .map(|tool| tool.name.clone())
        .collect()
}

fn lsp_first_nudge(call: &ToolCall, lsp_tools: &[String]) -> Option<ToolGuardrailNudge> {
    let tool_name = call.name.to_ascii_lowercase();
    let args = object_args(&call.input);
    let symbol = if is_shell_tool(&tool_name) {
        shell_grep_symbol(command_arg(args?)?)?
    } else if is_grep_tool(&tool_name) {
        string_arg(
            args?,
            &["symbol", "name", "pattern", "query", "regex", "needle"],
        )
        .and_then(symbol_from_search_value)?
    } else if is_glob_tool(&tool_name) {
        string_arg(args?, &["pattern", "query", "glob"]).and_then(symbol_from_search_value)?
    } else {
        return None;
    };

    let tools = lsp_tools.join(", ");
    let fingerprint = format!("lsp_first:{}:{symbol}", call.name);
    Some(ToolGuardrailNudge {
        call_id: call.id.clone(),
        kind: "lsp_first",
        content: format!(
            "Use available LSP tools for symbol lookup instead of grep/glob/shell search. Available LSP tools: {tools}. Retry with the best matching LSP tool for `{symbol}`."
        ),
        fingerprint,
    })
}

fn quiet_command_nudge(call: &ToolCall) -> Option<ToolGuardrailNudge> {
    let tool_name = call.name.to_ascii_lowercase();
    if !is_shell_tool(&tool_name) {
        return None;
    }
    let command = command_arg(object_args(&call.input)?)?.trim();
    let suggestion = quiet_command_suggestion(command)?;
    let fingerprint = format!("quiet:{}:{command}:{suggestion}", call.name);
    Some(ToolGuardrailNudge {
        call_id: call.id.clone(),
        kind: "quiet_command",
        content: format!(
            "The requested shell command is likely to produce noisy output. Prefer `{suggestion}`. Repeat the original command only if verbose output is required."
        ),
        fingerprint,
    })
}

fn write_payload_cap_nudge(call: &ToolCall, max_bytes: usize) -> Option<ToolGuardrailNudge> {
    // A cap of 0 disables the nudge rather than firing on every non-empty write.
    if max_bytes == 0 {
        return None;
    }
    let tool_name = call.name.to_ascii_lowercase();
    if !is_write_or_edit_tool(&tool_name) {
        return None;
    }
    let bytes = object_args(&call.input)
        .map(write_payload_bytes)
        .unwrap_or(0);
    if bytes <= max_bytes {
        return None;
    }
    Some(ToolGuardrailNudge {
        call_id: call.id.clone(),
        kind: "write_payload_cap",
        content: format!(
            "The requested write/edit payload is too large for this proxy policy ({bytes} bytes > {max_bytes} bytes). Retry with a smaller targeted edit or split the change."
        ),
        // Include a hash of the actual payload, not just its byte length, so
        // two unrelated oversized writes that happen to be the same size
        // (e.g. same-size templated files) don't collide and suppress each
        // other's nudge. Identical repeated calls (same target, same
        // content) still dedupe as intended.
        fingerprint: format!(
            "write_payload_cap:{}:{:x}:{bytes}:{max_bytes}",
            call.name,
            payload_content_hash(&call.input)
        ),
    })
}

fn payload_content_hash(value: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "bash" | "shell" | "run_command" | "execute_command" | "terminal" | "exec" | "execute_bash"
    )
}

fn is_grep_tool(name: &str) -> bool {
    matches!(name, "grep" | "rg" | "ripgrep" | "st")
}

fn is_glob_tool(name: &str) -> bool {
    matches!(name, "glob" | "find_files" | "file_glob")
}

fn is_write_or_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "write"
            | "write_file"
            | "edit"
            | "edit_file"
            | "replace"
            | "apply_patch"
            | "create_file"
            | "update_file"
    )
}

fn object_args(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn command_arg(args: &Map<String, Value>) -> Option<&str> {
    string_arg(args, &["command", "cmd", "shell_command", "input"])
}

fn string_arg<'a>(args: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    // Only the named keys, in order. Do NOT fall back to an arbitrary string
    // value: that would let an unrelated field be misread as the search symbol
    // or command for an advisory nudge.
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
}

fn shell_grep_symbol(command: &str) -> Option<String> {
    let mut parts = command.split_whitespace();
    let binary = strip_shell_quotes(parts.next()?)
        .rsplit('/')
        .next()?
        .to_string();
    if !matches!(binary.as_str(), "rg" | "ripgrep" | "grep" | "st") {
        return None;
    }

    let mut previous_took_value = false;
    for part in parts {
        let token = strip_shell_quotes(part);
        if previous_took_value {
            previous_took_value = false;
            continue;
        }
        if token.starts_with("--") {
            previous_took_value = matches!(
                token.as_str(),
                "--glob" | "--type" | "--context" | "--after-context" | "--before-context"
            );
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        return symbol_from_search_value(&token);
    }
    None
}

fn symbol_from_search_value(value: &str) -> Option<String> {
    let trimmed = strip_shell_quotes(value)
        .trim_matches('/')
        .replace("\\b", "")
        .replace(['^', '$'], "");
    for token in trimmed.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')) {
        if looks_like_symbol_token(token) {
            return Some(token.to_string());
        }
    }
    None
}

fn looks_like_symbol_token(token: &str) -> bool {
    if token.len() < 3 {
        return false;
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return false;
    }

    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "todo" | "fixme" | "error" | "warning" | "debug" | "test" | "src" | "main"
    ) {
        return false;
    }
    token.contains('_') || token.chars().any(|ch| ch.is_ascii_uppercase())
}

fn strip_shell_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn quiet_command_suggestion(command: &str) -> Option<String> {
    if command_has_prefix(command, "git log") && !contains_word(command, "--oneline") {
        return Some(insert_after_prefix(command, "git log", "--oneline"));
    }
    for prefix in ["cargo build", "cargo check", "cargo clippy", "cargo test"] {
        if command_has_prefix(command, prefix) && !contains_word(command, "--quiet") {
            return Some(insert_after_prefix(command, prefix, "--quiet"));
        }
    }
    if command_has_prefix(command, "pytest") && !contains_word(command, "-q") {
        return Some(insert_after_prefix(command, "pytest", "-q"));
    }
    if command_has_prefix(command, "npm install") && !contains_word(command, "--silent") {
        return Some(insert_after_prefix(command, "npm install", "--silent"));
    }
    if command_has_prefix(command, "pip install") && !contains_word(command, "--quiet") {
        return Some(insert_after_prefix(command, "pip install", "--quiet"));
    }
    if command_has_prefix(command, "docker build") && !contains_word(command, "--progress=quiet") {
        return Some(insert_after_prefix(
            command,
            "docker build",
            "--progress=quiet",
        ));
    }
    if command_has_prefix(command, "curl") && !contains_word(command, "-s") {
        return Some(insert_after_prefix(command, "curl", "-s"));
    }
    if command_has_prefix(command, "make") && !contains_word(command, "-s") {
        return Some(insert_after_prefix(command, "make", "-s"));
    }
    if command_has_prefix(command, "tree") && !contains_word(command, "-I") {
        return Some(insert_after_prefix(
            command,
            "tree",
            "-I \"node_modules|.git|target|dist|build\"",
        ));
    }
    None
}

fn command_has_prefix(command: &str, prefix: &str) -> bool {
    command == prefix || command.starts_with(&format!("{prefix} "))
}

fn contains_word(command: &str, word: &str) -> bool {
    command.split_whitespace().any(|part| part == word)
}

fn insert_after_prefix(command: &str, prefix: &str, insertion: &str) -> String {
    let rest = command[prefix.len()..].trim_start();
    if rest.is_empty() {
        format!("{prefix} {insertion}")
    } else {
        format!("{prefix} {insertion} {rest}")
    }
}

/// Depth cap for `value_payload_bytes`'s recursion. Model-controlled JSON has
/// no natural nesting limit; a real write/edit payload never needs anywhere
/// near this depth, so hitting it is itself a signal the payload is oversized.
const MAX_PAYLOAD_DEPTH: usize = 32;

fn write_payload_bytes(args: &Map<String, Value>) -> usize {
    let payload_keys = [
        "content",
        "text",
        "new_content",
        "patch",
        "diff",
        "replacement",
        "data",
        "old_string",
        "new_string",
    ];
    let known = args
        .iter()
        .filter(|(key, _)| payload_keys.contains(&key.as_str()))
        .fold(0usize, |acc, (_, value)| {
            acc.saturating_add(value_payload_bytes(value, 0))
        });
    if known > 0 {
        return known;
    }
    // None of the known field names matched (a tool with a non-standard
    // schema, e.g. `body`/`value`) -- fall back to summing every value in
    // the payload rather than silently reporting zero and never capping it.
    args.values().fold(0usize, |acc, value| {
        acc.saturating_add(value_payload_bytes(value, 0))
    })
}

fn value_payload_bytes(value: &Value, depth: usize) -> usize {
    if depth >= MAX_PAYLOAD_DEPTH {
        // Treat implausibly deep nesting as oversized rather than recursing
        // further; a fixed large sentinel (not usize::MAX) keeps the
        // saturating sum below safe from overflowing back to a small value.
        return 1_000_000_000;
    }
    match value {
        Value::String(value) => value.len(),
        Value::Array(values) => values.iter().fold(0usize, |acc, v| {
            acc.saturating_add(value_payload_bytes(v, depth + 1))
        }),
        Value::Object(values) => values.values().fold(0usize, |acc, v| {
            acc.saturating_add(value_payload_bytes(v, depth + 1))
        }),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_spec(name: &str) -> Tool {
        Tool {
            name: name.to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }
    }

    fn call(name: &str, input: Value) -> ToolCall {
        ToolCall {
            id: "toolu_1".to_string(),
            name: name.to_string(),
            input,
        }
    }

    #[test]
    fn lsp_nudge_requires_replacement_tool() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            lsp_first: true,
            ..ToolGuardrailConfig::disabled()
        };
        let calls = vec![call("grep", json!({"pattern": "UserService"}))];

        assert!(
            evaluate_tool_guardrails(&calls, &[tool_spec("grep")], &config, &mut state).is_empty()
        );

        let nudges = evaluate_tool_guardrails(
            &calls,
            &[tool_spec("grep"), tool_spec("find_definition")],
            &config,
            &mut state,
        );
        assert_eq!(nudges.len(), 1);
        let nudge = &nudges[0];
        assert_eq!(nudge.kind, "lsp_first");
        assert_eq!(nudge.call_id, "toolu_1");
        assert!(nudge.content.contains("find_definition"));
        assert!(nudge.content.contains("UserService"));
    }

    #[test]
    fn quiet_command_nudges_once() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            quiet_commands: true,
            ..ToolGuardrailConfig::disabled()
        };
        let calls = vec![call("bash", json!({"command": "cargo test"}))];
        let first = evaluate_tool_guardrails(&calls, &[], &config, &mut state);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, "quiet_command");
        assert!(first[0].content.contains("cargo test --quiet"));
        assert!(evaluate_tool_guardrails(&calls, &[], &config, &mut state).is_empty());
    }

    #[test]
    fn only_offending_call_is_nudged() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            lsp_first: true,
            ..ToolGuardrailConfig::disabled()
        };
        let calls = vec![
            ToolCall {
                id: "toolu_grep".into(),
                name: "grep".into(),
                input: json!({"pattern": "UserService"}),
            },
            ToolCall {
                id: "toolu_write".into(),
                name: "write_file".into(),
                input: json!({"content": "ok"}),
            },
        ];
        let nudges = evaluate_tool_guardrails(
            &calls,
            &[tool_spec("grep"), tool_spec("find_definition")],
            &config,
            &mut state,
        );
        assert_eq!(nudges.len(), 1);
        assert_eq!(nudges[0].call_id, "toolu_grep");
    }

    #[test]
    fn write_payload_cap_detects_oversized_payload() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            write_payload_caps: true,
            max_write_payload_bytes: 4,
            ..ToolGuardrailConfig::disabled()
        };
        let calls = vec![call("write_file", json!({"content": "12345"}))];
        let nudges = evaluate_tool_guardrails(&calls, &[], &config, &mut state);
        assert_eq!(nudges.len(), 1);
        assert_eq!(nudges[0].kind, "write_payload_cap");
        assert!(nudges[0].content.contains("5 bytes > 4 bytes"));
    }

    #[test]
    fn write_payload_cap_zero_disables_nudge() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            write_payload_caps: true,
            max_write_payload_bytes: 0,
            ..ToolGuardrailConfig::disabled()
        };
        let calls = vec![call("write_file", json!({"content": "12345"}))];
        assert!(evaluate_tool_guardrails(&calls, &[], &config, &mut state).is_empty());
    }

    #[test]
    fn write_payload_cap_does_not_collide_on_equal_length_different_content() {
        let mut state = ToolGuardrailRequestState::new();
        let config = ToolGuardrailConfig {
            write_payload_caps: true,
            max_write_payload_bytes: 4,
            ..ToolGuardrailConfig::disabled()
        };
        // Same byte length (5), different content/target -- both must nudge.
        let first = vec![call("write_file", json!({"content": "aaaaa"}))];
        let second = vec![call("write_file", json!({"content": "bbbbb"}))];
        assert_eq!(
            evaluate_tool_guardrails(&first, &[], &config, &mut state).len(),
            1
        );
        assert_eq!(
            evaluate_tool_guardrails(&second, &[], &config, &mut state).len(),
            1,
            "an unrelated oversized write of equal byte length must still be nudged"
        );
    }

    #[test]
    fn write_payload_bytes_falls_back_to_unrecognized_field_names() {
        let args = json!({"body": "12345"});
        assert_eq!(write_payload_bytes(args.as_object().unwrap()), 5);
    }

    #[test]
    fn resolve_runtime_guardrails_prefers_runtime_override() {
        let static_config = ToolGuardrailConfig::disabled();
        let resolved = resolve_runtime_guardrails(&static_config, "standard");
        assert_eq!(resolved, ToolGuardrailConfig::standard());
    }

    #[test]
    fn resolve_runtime_guardrails_keeps_static_when_modes_match() {
        let static_config = ToolGuardrailConfig {
            max_write_payload_bytes: 123,
            ..ToolGuardrailConfig::standard()
        };
        let resolved = resolve_runtime_guardrails(&static_config, "standard");
        // Same mode as the static preset -- the static config (with its
        // custom max_write_payload_bytes) must be preserved, not rebuilt
        // from the bare preset.
        assert_eq!(resolved, static_config);
    }

    #[test]
    fn resolve_runtime_guardrails_falls_back_on_unparseable_value() {
        let static_config = ToolGuardrailConfig::standard();
        let resolved = resolve_runtime_guardrails(&static_config, "not-a-real-mode");
        assert_eq!(resolved, static_config);
    }
}
