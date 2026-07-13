//! Anthropic Messages adapter. System is a TOP-LEVEL field (not a message) — Immutable,
//! never compressed, so it is not represented in the IR message list (keeping IR indices
//! 1:1 with the wire `messages` array). Content is a string or an array of blocks
//! (`text`, `tool_use`, `tool_result`, `image`, `thinking`). Prompt caching uses explicit
//! `cache_control` markers: any message carrying one is Immutable and never moved.

use anyllm_optimize_core::{ContentBlock, Conversation, Message, RenderedConversation};
use serde_json::Value;

use super::{parse_role, protection_for};

/// Build the IR from a parsed Anthropic request body. The top-level `system` is skipped
/// (Immutable, not in the message list). Non-text blocks (tool_use, tool_result, image,
/// thinking, unknown) map to `Opaque` and are never edited in this milestone.
pub fn from_value(root: &Value) -> Conversation {
    let msgs = root.get("messages").and_then(|m| m.as_array());
    let n = msgs.map(|a| a.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(n);

    for (i, m) in msgs.into_iter().flatten().enumerate() {
        let role = parse_role(m.get("role").and_then(|r| r.as_str()));
        let mut blocks = Vec::new();
        let mut client_marked = message_has_cache_control(m);

        match m.get("content") {
            Some(Value::String(s)) => blocks.push(ContentBlock::Text(s.clone())),
            Some(Value::Array(parts)) => {
                for p in parts {
                    if p.get("cache_control").is_some() {
                        client_marked = true;
                    }
                    match p.get("type").and_then(|t| t.as_str()) {
                        Some("text") => blocks.push(ContentBlock::Text(
                            p.get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string(),
                        )),
                        // tool_use / tool_result / image / thinking / unknown — passthrough.
                        _ => blocks.push(ContentBlock::Opaque { raw: p.to_string() }),
                    }
                }
            }
            _ => {}
        }

        out.push(Message {
            role,
            blocks,
            protection: protection_for(role, i, n, client_marked),
            client_cache_marker: client_marked,
        });
    }

    Conversation::new(out)
}

/// Write compressed text back into the original body by index, then (Live mode only,
/// when `rendered.breakpoint` is set) place the deepest explicit cache breakpoint.
/// Only `text` blocks are written; `system`, tool blocks, images, thinking, and
/// client-set `cache_control` are untouched.
pub fn apply_rendered(root: &mut Value, rendered: &RenderedConversation) {
    let Some(msgs) = root.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for (i, rmsg) in rendered.messages.iter().enumerate() {
        let Some(m) = msgs.get_mut(i) else { continue };
        match m.get("content") {
            Some(Value::String(_)) => {
                if let Some(ContentBlock::Text(t)) = rmsg.blocks.first() {
                    m["content"] = Value::String(t.clone());
                }
            }
            Some(Value::Array(_)) => {
                if let Some(arr) = m.get_mut("content").and_then(|c| c.as_array_mut()) {
                    for (bi, part) in arr.iter_mut().enumerate() {
                        if part.get("type").and_then(|t| t.as_str()) != Some("text") {
                            continue;
                        }
                        if let Some(ContentBlock::Text(t)) = rmsg.blocks.get(bi) {
                            part["text"] = Value::String(t.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // `rendered.breakpoint` is the frontier: messages `[0, breakpoint)` are the frozen
    // (compressed-eligible) zone that should become the cached prefix (ALGO.md §2,
    // `AnthropicStrategy::breakpoint_at`). Anthropic's `cache_control` marks "cache
    // everything up to and including this block", so the marker belongs on the LAST
    // message of that zone: index `breakpoint - 1`. `breakpoint == 0` means nothing is
    // frozen yet (nothing to cache); out-of-range values are ignored defensively
    // (fail-open — this is a belt-and-braces adapter, never trust caller data blindly).
    if let Some(bp) = rendered.breakpoint {
        if bp > 0 {
            if let Some(idx) = bp.checked_sub(1) {
                place_breakpoint(msgs, idx);
            }
        }
    }
}

/// Attach an ephemeral `cache_control` breakpoint to the last content block of the
/// message at `idx`. A string `"content"` is converted to a single-block array (the
/// wire format Anthropic requires for a block-level marker). Never duplicates or moves
/// a marker the client already placed on that block.
fn place_breakpoint(msgs: &mut [Value], idx: usize) {
    let Some(m) = msgs.get_mut(idx) else { return };
    match m.get("content") {
        Some(Value::String(text)) => {
            let text = text.clone();
            m["content"] = serde_json::json!([{
                "type": "text",
                "text": text,
                "cache_control": {"type": "ephemeral"},
            }]);
        }
        Some(Value::Array(_)) => {
            if let Some(last) = m
                .get_mut("content")
                .and_then(|c| c.as_array_mut())
                .and_then(|arr| arr.last_mut())
            {
                if last.get("cache_control").is_none() {
                    last["cache_control"] = serde_json::json!({"type": "ephemeral"});
                }
            }
        }
        _ => {}
    }
}

fn message_has_cache_control(m: &Value) -> bool {
    m.get("cache_control").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_optimize_core::{Protection, Role};
    use serde_json::json;

    #[test]
    fn skips_system_keeps_index_alignment() {
        let body = json!({
            "model": "claude-sonnet-5",
            "system": "you are helpful",
            "messages": [
                {"role":"user","content":"first"},
                {"role":"assistant","content":[{"type":"text","text":"reply"}]},
                {"role":"user","content":"latest"},
            ]
        });
        let conv = from_value(&body);
        assert_eq!(conv.len(), 3); // system NOT in the list
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[2].protection, Protection::Immutable); // latest
    }

    #[test]
    fn cache_control_marks_immutable() {
        let body = json!({
            "messages": [
                {"role":"user","content":[
                    {"type":"text","text":"cached ctx","cache_control":{"type":"ephemeral"}}
                ]},
                {"role":"user","content":"q"},
            ]
        });
        let conv = from_value(&body);
        assert!(conv.messages[0].client_cache_marker);
        assert_eq!(conv.messages[0].protection, Protection::Immutable);
    }

    #[test]
    fn roundtrips_when_no_edits() {
        let body = json!({
            "system": "sys",
            "messages": [
                {"role":"user","content":"keep me"},
                {"role":"assistant","content":[{"type":"text","text":"and me"}]},
            ]
        });
        let conv = from_value(&body);
        let rendered = RenderedConversation {
            messages: conv
                .messages
                .iter()
                .map(|m| anyllm_optimize_core::RenderedMessage {
                    blocks: m.blocks.clone(),
                })
                .collect(),
            breakpoint: None,
        };
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);
        assert_eq!(out, body);
    }

    /// `RenderedConversation` with `conv`'s own blocks unchanged, so these tests
    /// isolate breakpoint placement from text compression.
    fn unedited_rendered(conv: &Conversation, breakpoint: Option<usize>) -> RenderedConversation {
        RenderedConversation {
            messages: conv
                .messages
                .iter()
                .map(|m| anyllm_optimize_core::RenderedMessage {
                    blocks: m.blocks.clone(),
                })
                .collect(),
            breakpoint,
        }
    }

    #[test]
    fn breakpoint_converts_string_content_to_block_with_cache_control() {
        let body = json!({
            "messages": [
                {"role":"user","content":"first"},
                {"role":"assistant","content":"second"},
                {"role":"user","content":"latest"},
            ]
        });
        let conv = from_value(&body);
        // breakpoint == 2 (frontier): frozen zone is [0, 2), so the marker lands on
        // the LAST frozen message, index 1.
        let rendered = unedited_rendered(&conv, Some(2));
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);

        assert_eq!(
            out["messages"][1]["content"],
            json!([{"type":"text","text":"second","cache_control":{"type":"ephemeral"}}])
        );
        // Untouched: outside the frozen zone / not the boundary message.
        assert_eq!(out["messages"][0]["content"], json!("first"));
        assert_eq!(out["messages"][2]["content"], json!("latest"));
    }

    #[test]
    fn breakpoint_marks_last_block_of_array_content() {
        let body = json!({
            "messages": [
                {"role":"user","content":[
                    {"type":"text","text":"a"},
                    {"type":"text","text":"b"},
                ]},
                {"role":"user","content":"latest"},
            ]
        });
        let conv = from_value(&body);
        let rendered = unedited_rendered(&conv, Some(1));
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);

        assert!(out["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert_eq!(
            out["messages"][0]["content"][1]["cache_control"],
            json!({"type":"ephemeral"})
        );
    }

    #[test]
    fn breakpoint_zero_places_no_marker() {
        let body = json!({
            "messages": [
                {"role":"user","content":"only"},
            ]
        });
        let conv = from_value(&body);
        let rendered = unedited_rendered(&conv, Some(0));
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);
        assert_eq!(out, body, "frontier 0 means nothing is frozen yet");
    }

    #[test]
    fn breakpoint_never_duplicates_existing_client_cache_control() {
        let body = json!({
            "messages": [
                {"role":"user","content":[
                    {"type":"text","text":"a","cache_control":{"type":"ephemeral"}}
                ]},
                {"role":"user","content":"latest"},
            ]
        });
        let conv = from_value(&body);
        let rendered = unedited_rendered(&conv, Some(1));
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);
        assert_eq!(
            out, body,
            "client-set cache_control must not be touched/duplicated"
        );
    }

    #[test]
    fn breakpoint_out_of_range_is_ignored() {
        let body = json!({
            "messages": [
                {"role":"user","content":"only"},
            ]
        });
        let conv = from_value(&body);
        // breakpoint way beyond the message count — defensive, must not panic or index oob.
        let rendered = unedited_rendered(&conv, Some(50));
        let mut out = body.clone();
        apply_rendered(&mut out, &rendered);
        assert_eq!(out, body);
    }
}
