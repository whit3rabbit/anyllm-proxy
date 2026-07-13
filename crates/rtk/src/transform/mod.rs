//! IO-free request-body transforms. Walk the messages array on a
//! `serde_json::Value`, rewrite eligible tool-output content in place, and never
//! touch anything else. Ports the message walk in `index.ts::applyRtkCompression`.
//!
//! Cache safety (mirrors `hasCacheControlMarker`): any block or text sub-block
//! carrying a non-null `cache_control` is a client-declared prompt-cache
//! breakpoint and is preserved byte-for-byte — rewriting it would invalidate the
//! cached prefix every turn.

mod anthropic;
mod openai;

pub use anthropic::transform_anthropic;
pub use openai::transform_openai_chat;

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Outcome of a transform pass.
#[derive(Debug, Default, Clone)]
pub struct RtkInfo {
    /// Any block was rewritten to a smaller payload.
    pub compressed: bool,
    /// Count of tool-output text payloads rewritten.
    pub blocks_compressed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
}

/// tool id → (tool name, shell command if any) built from assistant tool calls.
pub(crate) type ToolLookup = HashMap<String, ToolMeta>;

#[derive(Clone)]
pub(crate) struct ToolMeta {
    pub name: String,
    /// Parsed shell command from `input.command` or `input.cmd` (Anthropic
    /// `tool_use.input` is a native Value; OpenAI `function.arguments` is a JSON
    /// string parsed lazily in `resolve_tool_meta` — see `arguments_str`).
    pub command: Option<String>,
    /// Raw JSON-string `function.arguments` from OpenAI tool calls. Parsed only
    /// for shell tools (deferred to `resolve_tool_meta`). None for Anthropic
    /// (where `input` is already a Value, so `command` is extracted directly).
    pub arguments_str: Option<String>,
}

fn shell_tool_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(bash|shell|terminal|run_command|execute_command|exec|command)\b").unwrap()
    })
}

/// Resolve `(command, skip_filters)` for a tool result — port of `resolveToolMeta`.
/// No lookup entry → run filters with text-based detection. Shell tool → use its
/// command. Non-shell tool (read/grep/glob/…) → skip filters.
pub(crate) fn resolve_tool_meta(
    tool_id: Option<&str>,
    lookup: &ToolLookup,
) -> (Option<String>, bool) {
    let meta = tool_id.and_then(|id| lookup.get(id));
    match meta {
        None => (None, false),
        Some(m) => {
            if shell_tool_re().is_match(&m.name.to_lowercase()) {
                // Get the command, parsing lazily from the raw OpenAI arguments
                // JSON string if not already extracted (Anthropic path extracts
                // command directly from the native `input` Value).
                let command = m.command.clone().or_else(|| {
                    m.arguments_str.as_deref().and_then(|a| {
                        serde_json::from_str::<Value>(a)
                            .ok()
                            .and_then(|args| command_from_input(&args))
                    })
                });
                (command, false)
            } else {
                (None, true)
            }
        }
    }
}

/// A block/sub-block with a non-null `cache_control` is preserved byte-for-byte.
pub(crate) fn has_cache_control(v: &Value) -> bool {
    matches!(v.get("cache_control"), Some(c) if !c.is_null())
}

/// Extract `command` / `cmd` from a tool-call input object.
pub(crate) fn command_from_input(input: &Value) -> Option<String> {
    input
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| input.get("cmd").and_then(Value::as_str))
        .map(str::to_string)
}
