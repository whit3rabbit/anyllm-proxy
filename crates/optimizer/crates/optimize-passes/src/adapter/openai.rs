//! OpenAI Chat Completions adapter. System is a `role:"system"` message in the array;
//! content is a string or an array of parts (`text`, `image_url`); tool calls are an
//! assistant `tool_calls[]` (args = JSON string, Immutable); tool results are a
//! `role:"tool"` message. Prompt caching is implicit prefix (no client markers).

use anyllm_optimize_core::{ContentBlock, Conversation, Message, RenderedConversation};
use serde_json::Value;

use super::{parse_role, protection_for, string_block};

/// Build the IR from a parsed OpenAI request body. Blocks are laid out 1:1 with the
/// content shape (one block per array part, or one for a string) followed by any
/// `tool_calls` as Immutable `ToolUse` blocks — so `apply_rendered` can map back by
/// index.
pub fn from_value(root: &Value) -> Conversation {
    let msgs = root.get("messages").and_then(|m| m.as_array());
    let n = msgs.map(|a| a.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(n);

    for (i, m) in msgs.into_iter().flatten().enumerate() {
        let role = parse_role(m.get("role").and_then(|r| r.as_str()));
        let mut blocks = Vec::new();

        match m.get("content") {
            Some(Value::String(s)) => blocks.push(string_block(role, s)),
            Some(Value::Array(parts)) => {
                for p in parts {
                    match p.get("type").and_then(|t| t.as_str()) {
                        Some("text") => blocks.push(ContentBlock::Text(
                            p.get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string(),
                        )),
                        // image_url / input_audio / anything else — passthrough.
                        _ => blocks.push(ContentBlock::Opaque { raw: p.to_string() }),
                    }
                }
            }
            _ => {}
        }

        if let Some(tc) = m.get("tool_calls").and_then(|t| t.as_array()) {
            for c in tc {
                blocks.push(ContentBlock::ToolUse { raw: c.to_string() });
            }
        }

        out.push(Message {
            role,
            blocks,
            protection: protection_for(role, i, n, false),
            client_cache_marker: false,
        });
    }

    Conversation::new(out)
}

/// Write compressed text back into the original body by index. Only Text/ToolResult
/// blocks are written; every other field is left byte-identical.
pub fn apply_rendered(root: &mut Value, rendered: &RenderedConversation) {
    let Some(msgs) = root.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for (i, rmsg) in rendered.messages.iter().enumerate() {
        let Some(m) = msgs.get_mut(i) else { continue };
        match m.get("content") {
            Some(Value::String(_)) => {
                if let Some(text) = block_text(rmsg.blocks.first()) {
                    m["content"] = Value::String(text.to_string());
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
}

fn block_text(b: Option<&ContentBlock>) -> Option<&str> {
    match b {
        Some(ContentBlock::Text(s)) | Some(ContentBlock::ToolResult { raw: s }) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_optimize_core::Role;
    use serde_json::json;

    #[test]
    fn parses_roles_and_content() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "you are helpful"},
                {"role": "user", "content": "hi there"},
                {"role": "assistant", "content": [{"type":"text","text":"hello"}]},
            ]
        });
        let conv = from_value(&body);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, Role::System);
        assert_eq!(conv.messages[1].role, Role::User);
        // last message is Immutable
        assert_eq!(
            conv.messages[2].protection,
            anyllm_optimize_core::Protection::Immutable
        );
    }

    #[test]
    fn roundtrips_when_no_edits() {
        // apply_rendered with identical blocks must leave the body unchanged.
        let body = json!({
            "messages": [
                {"role":"user","content":"keep me exactly"},
                {"role":"user","content":[{"type":"text","text":"and me"}]},
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
}
