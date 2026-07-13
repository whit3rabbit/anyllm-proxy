//! OpenAI Chat Completions request transform. Compresses the content of
//! `role: "tool"` messages, resolving each tool's shell command from the
//! preceding assistant `tool_calls`.

use super::{has_cache_control, resolve_tool_meta, RtkInfo, ToolLookup, ToolMeta};
use crate::engine::process_rtk_text;
use serde_json::Value;

/// Build tool_call_id → metadata from assistant `tool_calls[]`
/// (`function.name`, `function.arguments` JSON → `command`/`cmd`).
fn build_lookup(messages: &[Value]) -> ToolLookup {
    let mut lookup = ToolLookup::new();
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for tc in calls {
            let Some(id) = tc.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(func) = tc.get("function") else {
                continue;
            };
            let name = func
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // Store the raw arguments JSON string; defer JSON parse to
            // `resolve_tool_meta`, which only parses for shell tools.
            let arguments_str = func
                .get("arguments")
                .and_then(Value::as_str)
                .map(str::to_string);
            // Extract the command eagerly for Anthropic-style native Values
            // (the Anthropic path extracts from native `input`, not from JSON
            // strings). For OpenAI the parse is deferred — build_lookup only
            // stores the raw string.
            let command = None;
            lookup.insert(
                id.to_string(),
                ToolMeta {
                    name,
                    command,
                    arguments_str,
                },
            );
        }
    }
    lookup
}

/// Compress `role: "tool"` message content in place.
pub fn transform_openai_chat(root: &mut Value) -> RtkInfo {
    let mut info = RtkInfo::default();

    // Build the tool lookup from an immutable borrow, then drop it before
    // re-borrowing mutably — avoids cloning the entire messages array.
    let lookup = {
        let Some(messages) = root.get("messages").and_then(Value::as_array) else {
            return info;
        };
        build_lookup(messages)
    };

    let Some(messages) = root.get_mut("messages").and_then(Value::as_array_mut) else {
        return info;
    };
    let info_out = &mut info;

    for msg in messages.iter_mut() {
        if msg.get("role").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let tool_call_id = msg
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let (command, skip_filters) = resolve_tool_meta(tool_call_id.as_deref(), &lookup);

        let Some(content) = msg.get_mut("content") else {
            continue;
        };
        match content {
            Value::String(s) => {
                if s.is_empty() {
                    continue;
                }
                let res = process_rtk_text(s, command.as_deref(), skip_filters);
                if res.compressed {
                    info_out.blocks_compressed += 1;
                    info_out.chars_before += s.chars().count();
                    info_out.chars_after += res.text.chars().count();
                    *s = res.text;
                }
            }
            Value::Array(parts) => {
                for part in parts.iter_mut() {
                    if part.get("type").and_then(Value::as_str) != Some("text") {
                        continue;
                    }
                    if has_cache_control(part) {
                        continue;
                    }
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    if text.is_empty() {
                        continue;
                    }
                    let res = process_rtk_text(text, command.as_deref(), skip_filters);
                    if res.compressed {
                        info_out.blocks_compressed += 1;
                        info_out.chars_before += text.chars().count();
                        info_out.chars_after += res.text.chars().count();
                        part["text"] = Value::String(res.text);
                    }
                }
            }
            _ => {}
        }
    }

    info_out.compressed = info_out.blocks_compressed > 0;
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compresses_openai_tool_message() {
        let mut noise = String::from("On branch main\nChanges not staged for commit:\n");
        for i in 0..200 {
            noise.push_str(&format!("  (use \"git add ...\" file {i})\n"));
        }
        let mut body = json!({
            "messages": [
                {"role": "assistant", "tool_calls": [
                    {"id": "c1", "type": "function",
                     "function": {"name": "bash", "arguments": "{\"command\":\"git status\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "c1", "content": noise.clone()}
            ]
        });
        let info = transform_openai_chat(&mut body);
        assert!(info.compressed);
        let out = body["messages"][1]["content"].as_str().unwrap();
        assert!(out.contains("On branch main"));
        assert!(out.len() < noise.len());
    }
}
