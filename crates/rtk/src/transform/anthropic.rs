//! Anthropic Messages request transform. Compresses the inner text of
//! `tool_result` content blocks (`applyToToolResults` default), resolving each
//! block's shell command from the matching assistant `tool_use`.

use super::{
    command_from_input, has_cache_control, resolve_tool_meta, RtkInfo, ToolLookup, ToolMeta,
};
use crate::engine::process_rtk_text;
use serde_json::Value;

/// Build tool_use_id → metadata from assistant `content[]` `tool_use` blocks.
fn build_lookup(messages: &[Value]) -> ToolLookup {
    let mut lookup = ToolLookup::new();
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(blocks) = msg.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in blocks {
            if part.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let Some(id) = part.get("id").and_then(Value::as_str) else {
                continue;
            };
            let name = part
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let command = part.get("input").and_then(command_from_input);
            lookup.insert(
                id.to_string(),
                ToolMeta {
                    name,
                    command,
                    arguments_str: None,
                },
            );
        }
    }
    lookup
}

/// Compress `tool_result` blocks in place. Returns what changed.
pub fn transform_anthropic(root: &mut Value) -> RtkInfo {
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
        let Some(blocks) = msg.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks.iter_mut() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            // Cache-breakpoint block: never rewrite.
            if has_cache_control(block) {
                continue;
            }
            let tool_use_id = block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let (command, skip_filters) = resolve_tool_meta(tool_use_id.as_deref(), &lookup);

            let Some(inner) = block.get_mut("content") else {
                continue;
            };
            compress_tool_result_content(inner, command.as_deref(), skip_filters, info_out);
        }
    }

    info_out.compressed = info_out.blocks_compressed > 0;
    info
}

/// The `content` of a tool_result is a string OR an array of text blocks.
fn compress_tool_result_content(
    inner: &mut Value,
    command: Option<&str>,
    skip_filters: bool,
    info: &mut RtkInfo,
) {
    match inner {
        Value::String(s) => {
            if s.is_empty() {
                return;
            }
            let res = process_rtk_text(s, command, skip_filters);
            if res.compressed {
                info.blocks_compressed += 1;
                info.chars_before += s.chars().count();
                info.chars_after += res.text.chars().count();
                *s = res.text;
            }
        }
        Value::Array(subs) => {
            for sub in subs.iter_mut() {
                if sub.get("type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                // A text sub-block can carry its own cache breakpoint.
                if has_cache_control(sub) {
                    continue;
                }
                let Some(text) = sub.get("text").and_then(Value::as_str) else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                let res = process_rtk_text(text, command, skip_filters);
                if res.compressed {
                    info.blocks_compressed += 1;
                    info.chars_before += text.chars().count();
                    info.chars_after += res.text.chars().count();
                    sub["text"] = Value::String(res.text);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn big_git_status() -> String {
        let mut s = String::from("On branch main\nChanges not staged for commit:\n");
        for i in 0..200 {
            s.push_str(&format!("  (use \"git add ...\" to update file {i})\n"));
        }
        s.push_str("\tmodified: src/app.ts\n");
        s
    }

    #[test]
    fn compresses_tool_result_and_preserves_cache_control() {
        let mut body = json!({
            "model": "claude-sonnet-5",
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "bash", "input": {"command": "git status"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": big_git_status()},
                    {"type": "tool_result", "tool_use_id": "t1", "content": "cached noise",
                     "cache_control": {"type": "ephemeral"}}
                ]}
            ]
        });
        let info = transform_anthropic(&mut body);
        assert!(info.compressed);
        assert_eq!(info.blocks_compressed, 1);

        let blocks = body["messages"][1]["content"].as_array().unwrap();
        // First block compressed (shorter), keeps the branch line.
        let first = blocks[0]["content"].as_str().unwrap();
        assert!(first.contains("On branch main"));
        assert!(first.len() < big_git_status().len());
        // Cache-control block untouched.
        assert_eq!(blocks[1]["content"].as_str().unwrap(), "cached noise");
        assert!(blocks[1].get("cache_control").is_some());
    }

    #[test]
    fn non_shell_tool_skips_filters() {
        // A Read tool returning a .ts file must NOT be run through build-typescript.
        let file = (0..300)
            .map(|i| format!("const x{i} = {i}; // line"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "r1", "name": "Read", "input": {"file_path": "a.ts"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "r1", "content": file.clone()}
                ]}
            ]
        });
        let _info = transform_anthropic(&mut body);
        // skip_filters + no dedup/truncation change on unique lines -> unchanged.
        assert_eq!(
            body["messages"][1]["content"][0]["content"]
                .as_str()
                .unwrap(),
            file
        );
    }
}
