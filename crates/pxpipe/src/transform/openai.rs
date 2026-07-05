//! OpenAI Chat Completions request transform. Port of the static-slab path of
//! pxpipe's `openai.ts` (Chat form only).
//!
//! Simpler than the Anthropic path in one key way: OpenAI has no `cache_control`
//! breakpoints (prompt caching is automatic/prefix-based), so there is no anchor
//! to relocate — we just replace the system/developer slab with a pointer + image
//! and strip the tool docs. Value-based like the Anthropic transform; the proxy
//! round-trips this through the typed `ChatCompletionRequest` (which has a
//! `#[serde(flatten)] extra` catch-all, so the round-trip is lossless).
//!
//! Scope: static system/developer + tool docs. GPT history collapse and the
//! Responses API shape are not ported here (follow-ups).

use base64::Engine;
use serde_json::{json, Value};

use super::info::TransformInfo;
use super::{factsheet, gate, schema_strip};
use crate::render::{render_text, RenderOpts};

#[derive(Clone, Copy, Debug)]
pub struct GptOpts {
    /// Narrower than the Anthropic slab: 150 cols × 5px + pad ≈ 768px, OpenAI's
    /// shortest-side floor, so dense text isn't downscaled below legibility.
    pub cols: usize,
    /// OpenAI permits a taller portrait strip than Anthropic (2048-box resize).
    pub max_height_px: usize,
    pub chars_per_token: f64,
    pub min_compress_chars: usize,
    pub compress_tools: bool,
}

impl Default for GptOpts {
    fn default() -> Self {
        Self {
            cols: 150,
            max_height_px: 2048,
            chars_per_token: 4.0,
            min_compress_chars: 6_000,
            compress_tools: true,
        }
    }
}

const SLAB_HEADER: &str =
    "Reference context for this session, rendered as an image. Read it as text.\n\n";

/// Text of an OpenAI message `content` (string or array of `text` parts).
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for p in parts {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t);
                    }
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Full doc for one OpenAI function tool (prose + compact params schema).
fn render_tool_doc(tool: &Value) -> String {
    let f = tool.get("function").unwrap_or(tool);
    let name = f.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let desc = f.get("description").and_then(|d| d.as_str()).unwrap_or("");
    let schema = f
        .get("parameters")
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .unwrap_or_default();
    format!("## Tool: {name}\n{desc}\n```json\n{schema}\n```\n")
}

/// OpenAI image content part from PNG bytes.
fn image_part(png: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    json!({
        "type": "image_url",
        "image_url": { "url": format!("data:image/png;base64,{data}"), "detail": "high" }
    })
}

/// Transform an OpenAI Chat Completions body in place. `root` unchanged on skip.
pub fn transform(root: &mut Value, opts: &GptOpts) -> TransformInfo {
    let Some(messages) = root.get("messages").and_then(|m| m.as_array()) else {
        return TransformInfo::skipped("parse_error");
    };

    // Collect system/developer slab text + the indices of those messages.
    let mut system_text = String::new();
    let mut system_idxs: Vec<usize> = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        match m.get("role").and_then(|r| r.as_str()) {
            Some("system") | Some("developer") => {
                let t = content_text(m.get("content"));
                if !t.is_empty() {
                    if !system_text.is_empty() {
                        system_text.push('\n');
                    }
                    system_text.push_str(&t);
                }
                system_idxs.push(i);
            }
            _ => {}
        }
    }

    let tools_present = opts.compress_tools
        && root
            .get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|a| !a.is_empty());
    let mut tool_ref = String::new();
    if tools_present {
        if let Some(arr) = root.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                tool_ref.push_str(&render_tool_doc(t));
            }
        }
    }

    let mut slab = String::with_capacity(system_text.len() + tool_ref.len() + 64);
    slab.push_str(SLAB_HEADER);
    slab.push_str(&system_text);
    if !tool_ref.is_empty() {
        slab.push_str("\n\n# Tool Reference\n");
        slab.push_str(&tool_ref);
    }

    if system_text.trim().is_empty() && tool_ref.is_empty() {
        return TransformInfo::skipped("no_slab");
    }
    let slab_chars = slab.chars().count();
    if slab_chars < opts.min_compress_chars {
        return TransformInfo::skipped("below_min_chars");
    }

    let images = render_text(
        &slab,
        RenderOpts {
            cols: opts.cols,
            max_height_px: opts.max_height_px,
        },
    );
    if !gate::is_gpt_profitable(&images, slab_chars, opts.chars_per_token) {
        return TransformInfo::skipped("not_profitable");
    }

    // ---- commit -------------------------------------------------------------
    let fact = factsheet::fact_sheet_text(&slab);
    let image_parts: Vec<Value> = images.iter().map(|im| image_part(&im.png)).collect();

    let obj = root.as_object_mut().expect("messages implies object");
    let msgs = obj
        .get_mut("messages")
        .and_then(|m| m.as_array_mut())
        .expect("messages array");

    // 1. Replace each system/developer message content with a short pointer; the
    //    first also carries the fact-sheet.
    for (n, &idx) in system_idxs.iter().enumerate() {
        let mut pointer = String::from(
            "[Your reference context for this session is provided as an image in the first user message. Read it there.]",
        );
        if n == 0 && !fact.is_empty() {
            pointer.push('\n');
            pointer.push_str(&fact);
        }
        if let Some(mm) = msgs.get_mut(idx).and_then(|m| m.as_object_mut()) {
            mm.insert("content".into(), Value::String(pointer));
        }
    }

    // 2. Prepend image parts to the first user message.
    prepend_images_to_first_user(msgs, image_parts);

    // 3. Strip tool docs: stub description, annotation-strip parameters.
    if tools_present {
        if let Some(arr) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
            for t in arr.iter_mut() {
                let Some(f) = t.get_mut("function").and_then(|f| f.as_object_mut()) else {
                    continue;
                };
                let name = f
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                f.insert(
                    "description".into(),
                    Value::String(format!("See \"## Tool: {name}\" in the reference image.")),
                );
                if let Some(params) = f.get("parameters") {
                    let stripped = schema_strip::strip(params);
                    if schema_strip::has_structure(&stripped) {
                        f.insert("parameters".into(), stripped);
                    }
                }
            }
        }
    }

    // OpenAI has no cache_control anchor, so relocated_cache_anchor stays false
    // (its Default) — no reassignment needed.
    TransformInfo {
        compressed: true,
        reason: "applied",
        compressed_chars: slab.len(),
        image_count: images.len(),
        image_bytes: images.iter().map(|im| im.png.len()).sum(),
        image_pixels: images
            .iter()
            .map(|im| im.width as usize * im.height as usize)
            .sum(),
        dropped_chars: images.iter().map(|im| im.dropped).sum(),
        ..Default::default()
    }
}

/// Prepend `parts` to the first user message's content (string → parts array).
/// If there is NO user message, insert a fresh one holding the parts — otherwise
/// the system slab (already rewritten to a pointer) would reference an image that
/// was never attached, permanently losing the system prompt.
fn prepend_images_to_first_user(msgs: &mut Vec<Value>, parts: Vec<Value>) {
    let Some(idx) = msgs
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    else {
        msgs.insert(0, json!({ "role": "user", "content": parts }));
        return;
    };
    let Some(mm) = msgs[idx].as_object_mut() else {
        return;
    };
    match mm.get_mut("content") {
        Some(Value::String(s)) => {
            let text = std::mem::take(s);
            let mut new = parts;
            new.push(json!({ "type": "text", "text": text }));
            mm.insert("content".into(), Value::Array(new));
        }
        Some(Value::Array(existing)) => {
            let mut new = parts;
            new.append(existing);
            *existing = new;
        }
        _ => {
            mm.insert("content".into(), Value::Array(parts));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big_system() -> String {
        let mut s = String::from("Rules. See src/app.rs and API_KEY env, flag --fast.\n");
        s.push_str(&"Follow the instructions precisely and carefully. ".repeat(320));
        s
    }

    #[test]
    fn images_system_and_stubs_tools() {
        let mut root = json!({
            "model": "gpt-5.6",
            "messages": [
                { "role": "system", "content": big_system() },
                { "role": "user", "content": "do the task" }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "run",
                    "description": "long ".repeat(60),
                    "parameters": { "type": "object", "description": "x", "properties": { "cmd": { "type": "string" } }, "required": ["cmd"] }
                }
            }]
        });
        let info = transform(&mut root, &GptOpts::default());
        assert!(info.compressed, "reason={}", info.reason);
        // system replaced by pointer.
        assert!(root["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("first user message"));
        // first user message leads with an image_url part.
        let uc = root["messages"][1]["content"].as_array().unwrap();
        assert_eq!(uc[0]["type"], "image_url");
        assert!(uc[0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(uc.last().unwrap()["text"], "do the task");
        // tool stubbed + params kept structural.
        let f = &root["tools"][0]["function"];
        assert!(f["description"].as_str().unwrap().contains("## Tool: run"));
        assert!(f["parameters"].get("description").is_none());
        assert_eq!(f["parameters"]["required"], json!(["cmd"]));
    }

    #[test]
    fn tiny_passes_through() {
        let mut root = json!({
            "model": "gpt-5.6",
            "messages": [
                { "role": "system", "content": "be nice" },
                { "role": "user", "content": "hi" }
            ]
        });
        let before = root.clone();
        let info = transform(&mut root, &GptOpts::default());
        assert!(!info.compressed);
        assert_eq!(root, before);
    }

    #[test]
    fn inserts_user_message_when_none_exists() {
        // System-only request: the slab is imaged, but with no user message the
        // images must be attached to a freshly inserted one (not dropped, which
        // would leave a pointer to a nonexistent image and lose the system prompt).
        let mut root = json!({
            "model": "gpt-5.6",
            "messages": [ { "role": "system", "content": big_system() } ]
        });
        let info = transform(&mut root, &GptOpts::default());
        assert!(info.compressed, "reason={}", info.reason);
        let msgs = root["messages"].as_array().unwrap();
        let user = msgs
            .iter()
            .find(|m| m["role"] == "user")
            .expect("a user message must be inserted to hold the images");
        assert_eq!(user["content"].as_array().unwrap()[0]["type"], "image_url");
    }

    #[test]
    fn deterministic() {
        let make = || {
            json!({
                "model": "gpt-5.6",
                "messages": [
                    { "role": "system", "content": big_system() },
                    { "role": "user", "content": "go" }
                ]
            })
        };
        let (mut a, mut b) = (make(), make());
        transform(&mut a, &GptOpts::default());
        transform(&mut b, &GptOpts::default());
        assert_eq!(a, b);
    }
}
