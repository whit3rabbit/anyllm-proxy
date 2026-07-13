use super::common::{image_block, tool_result_text, AnthropicOpts, HISTORY_INTRO, HISTORY_OUTRO};
use crate::render::{render_text, RenderOpts};
use crate::transform::gate;
use crate::transform::info::TransformInfo;
use serde_json::{json, Value};

/// Collapse the OLD closed-tool-call message prefix into ONE synthetic user
/// message holding history image(s); keep the recent tail as text.
///
/// **Cache stability** is the whole risk here. Two guarantees keep the rendered
/// PNG byte-identical across turns so Anthropic prompt-caches it instead of
/// re-creating it every turn: (1) the collapse boundary is snapped DOWN to a
/// `history_collapse_chunk` message grid, so it only advances in steps and the
/// serialized text is stable for a whole window; (2) the serializer and renderer
/// are pure functions of the message bytes (no timestamps/rng, thinking blocks
/// dropped deterministically). If either breaks, this NET-LOSES money — hence
/// default-off until validated live.
///
/// **Correctness**: only a tool-CLOSED prefix is collapsed (every `tool_use` has
/// its matching `tool_result` within the range), so no tool call is ever
/// orphaned. The first user message (which carries the slab images) is protected.
///
/// NOTE: the synthetic message is role `user`, which can place it adjacent to the
/// protected first user message. The Anthropic Messages API accepts consecutive
/// same-role messages (pxpipe relies on this in production); images require the
/// user role, so this is unavoidable for a history-image message.
pub(crate) fn apply_history(
    root: &mut Value,
    opts: &AnthropicOpts,
    info: &mut TransformInfo,
    budget: &mut usize,
) {
    let Some(messages) = root.get("messages").and_then(|m| m.as_array()) else {
        return;
    };
    let len = messages.len();
    // Protect the slab-bearing first user message: collapse starts after it.
    let Some(first_user) = messages
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    else {
        return;
    };
    let protected = first_user + 1;
    let cutoff = len.saturating_sub(opts.keep_tail_messages);
    if cutoff <= protected {
        return;
    }
    let Some(boundary) = find_closed_boundary(messages, cutoff, protected) else {
        return;
    };
    // Snap DOWN to the chunk grid (relative to `protected`) for byte-stability.
    let chunk = opts.history_collapse_chunk.max(1);
    let grid = protected + ((boundary - protected) / chunk) * chunk;
    // The grid line is NOT guaranteed tool-closed (parity shifts from text-only
    // turns or parallel tool spans can leave it mid-open-span). Re-snap to the
    // largest CLOSED boundary <= the grid line so no tool_use is orphaned into
    // the history image; correctness beats the grid's cache-stability here.
    let Some(snapped) = find_closed_boundary(messages, grid, protected) else {
        return;
    };
    if snapped.saturating_sub(protected) < opts.min_collapse_prefix_messages {
        return;
    }

    let text = messages_to_history_text(messages, protected, snapped);
    if text.trim().is_empty() {
        return;
    }
    let images = render_text(
        &text,
        RenderOpts {
            cols: opts.cols,
            max_height_px: opts.max_height_px,
        },
    );
    if !gate::is_profitable(&images, text.chars().count(), opts.chars_per_token) {
        return;
    }
    if images.len() > *budget {
        return;
    }
    *budget -= images.len();

    // Build the synthetic message content: intro, images, outro.
    let mut content: Vec<Value> = Vec::with_capacity(images.len() + 2);
    content.push(json!({ "type": "text", "text": HISTORY_INTRO }));
    content.extend(images.iter().map(|im| image_block(&im.png)));
    content.push(json!({ "type": "text", "text": HISTORY_OUTRO }));
    let synthetic = json!({ "role": "user", "content": content });

    let collapsed_turns = snapped - protected;
    info.collapsed_turns = collapsed_turns;
    info.collapsed_chars = text.len();
    info.collapsed_images = images.len();
    info.image_count += images.len();
    info.image_bytes += images.iter().map(|im| im.png.len()).sum::<usize>();
    info.image_pixels += images
        .iter()
        .map(|im| im.width as usize * im.height as usize)
        .sum::<usize>();
    info.dropped_chars += images.iter().map(|im| im.dropped).sum::<usize>();
    info.compressed_chars += text.len();

    // Splice: [0..protected] + synthetic + [snapped..].
    let arr = root
        .get_mut("messages")
        .and_then(|m| m.as_array_mut())
        .expect("messages was an array above");
    arr.splice(protected..snapped, std::iter::once(synthetic));
}

/// Largest exclusive end `e` in `(from, cutoff]` where messages `[from..e)` open
/// no tool call they don't also close. Returns `None` if none exists. Robust to
/// interleaved/parallel tool calls via the open-id set.
fn find_closed_boundary(messages: &[Value], cutoff: usize, from: usize) -> Option<usize> {
    let mut open: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_closed = None;
    for (i, m) in messages.iter().enumerate().take(cutoff).skip(from) {
        if let Some(blocks) = m.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        if let Some(id) = b.get("id").and_then(|i| i.as_str()) {
                            open.insert(id.to_string());
                        }
                    }
                    Some("tool_result") => {
                        if let Some(id) = b.get("tool_use_id").and_then(|i| i.as_str()) {
                            open.remove(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        if open.is_empty() {
            last_closed = Some(i + 1);
        }
    }
    last_closed
}

/// Serialize messages `[from..to)` to `<role>…</role>` XML text. thinking blocks
/// dropped; tool_use/tool_result flattened; inline images become `[image]`.
fn messages_to_history_text(messages: &[Value], from: usize, to: usize) -> String {
    let mut out = String::new();
    for m in &messages[from..to] {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let body = flatten_content(m.get("content"));
        out.push('<');
        out.push_str(role);
        out.push_str(">\n");
        out.push_str(&body);
        out.push_str("\n</");
        out.push_str(role);
        out.push_str(">\n");
    }
    out
}

/// Flatten one message's content to text: text verbatim, tool_use/tool_result to
/// a compact marker, thinking dropped, inline images to `[image]`.
fn flatten_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<String> = Vec::new();
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let input = b
                            .get("input")
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .unwrap_or_default();
                        parts.push(format!("[tool_use {name} {input}]"));
                    }
                    Some("tool_result") => {
                        parts.push(format!(
                            "[tool_result {}]",
                            tool_result_text(b.get("content"))
                        ));
                    }
                    Some("image") => parts.push("[image]".to_string()),
                    // thinking / redacted_thinking / unknown: dropped.
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}
