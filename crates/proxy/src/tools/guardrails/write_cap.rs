use super::utils::*;
use super::ToolGuardrailNudge;
use crate::tools::execution::ToolCall;
use serde_json::{Map, Value};

/// Depth cap for `value_payload_bytes`'s recursion. Model-controlled JSON has
/// no natural nesting limit; a real write/edit payload never needs anywhere
/// near this depth, so hitting it is itself a signal the payload is oversized.
const MAX_PAYLOAD_DEPTH: usize = 32;

pub(super) fn write_payload_cap_nudge(
    call: &ToolCall,
    max_bytes: usize,
) -> Option<ToolGuardrailNudge> {
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

pub(super) fn write_payload_bytes(args: &Map<String, Value>) -> usize {
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
