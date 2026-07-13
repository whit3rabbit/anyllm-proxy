// Common translation utilities shared across the Gemini message-map directions:
// tool-id lookup and Gemini role-alternation merging.

use std::collections::HashMap;

use crate::anthropic::messages as anthropic;
use crate::gemini::request as gemini;

/// Build a map from Anthropic tool_use IDs to tool names.
///
/// Scans all messages for `ToolUse` blocks so that `ToolResult` translation can
/// look up the function name Gemini expects.
pub fn build_tool_id_map(messages: &[anthropic::InputMessage]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for msg in messages {
        let blocks = match &msg.content {
            anthropic::Content::Text(_) => continue,
            anthropic::Content::Blocks(b) => b,
        };
        for block in blocks {
            if let anthropic::ContentBlock::ToolUse { id, name, .. } = block {
                map.insert(id.clone(), name.clone());
            }
        }
    }
    map
}

/// Merge consecutive same-role `Content` entries by concatenating their parts.
///
/// Gemini requires strict user/model role alternation. When the Anthropic
/// conversation has two consecutive user (or model) turns, this merges them
/// into a single turn.
pub fn merge_consecutive_roles(contents: Vec<gemini::Content>) -> Vec<gemini::Content> {
    let mut merged: Vec<gemini::Content> = Vec::with_capacity(contents.len());
    for c in contents {
        if let Some(last) = merged.last_mut() {
            if last.role == c.role {
                last.parts.extend(c.parts);
                continue;
            }
        }
        merged.push(c);
    }

    // Gemini requires the first content turn to have role "user". An Anthropic
    // client may legally send an assistant-first conversation (for few-shot
    // prompting). Prepend a dummy user turn so Gemini does not return a 400.
    if merged.first().and_then(|c| c.role.as_deref()) == Some("model") {
        merged.insert(
            0,
            gemini::Content {
                role: Some("user".to_string()),
                parts: vec![gemini::Part::text(String::new())],
            },
        );
    }

    merged
}
