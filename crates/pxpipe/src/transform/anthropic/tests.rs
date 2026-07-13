use super::common::AnthropicOpts;
use super::transform;
use serde_json::json;

fn big_system() -> String {
    // ~14k chars with some notable identifiers for the factsheet.
    let mut s = String::from("Operating rules. See src/main.rs and CONFIG_PATH env.\n");
    s.push_str(&"Do the thing carefully and precisely. ".repeat(320));
    s.push_str(" commit deadbeef1 flag --verbose");
    s
}

#[test]
fn images_system_and_relocates_anchor() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": [
            { "type": "text", "text": big_system(), "cache_control": { "type": "ephemeral" } }
        ],
        "messages": [ { "role": "user", "content": "hi there" } ]
    });
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(info.compressed, "reason={}", info.reason);
    assert!(info.relocated_cache_anchor);

    // system is now a short pointer string.
    assert!(root["system"].is_string());
    assert!(root["system"]
        .as_str()
        .unwrap()
        .contains("first user message"));

    // first user message leads with an image block carrying the anchor.
    let content = root["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content.last().unwrap()["type"], "text"); // original "hi there"
    assert_eq!(content.last().unwrap()["text"], "hi there");
    // anchor is on exactly the last image, nowhere in system.
    let last_img = content.iter().rev().find(|b| b["type"] == "image").unwrap();
    assert!(last_img.get("cache_control").is_some());
}

#[test]
fn tiny_system_passes_through_unchanged() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "be helpful",
        "messages": [ { "role": "user", "content": "hi" } ]
    });
    let before = root.clone();
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(!info.compressed);
    assert_eq!(info.reason, "below_min_chars");
    assert_eq!(root, before, "skip path must not mutate the body");
}

#[test]
fn same_input_same_output_cache_stable() {
    let make = || {
        json!({
            "model": "claude-fable-5",
            "system": big_system(),
            "messages": [ { "role": "user", "content": "go" } ]
        })
    };
    let mut a = make();
    let mut b = make();
    transform(&mut a, &AnthropicOpts::default());
    transform(&mut b, &AnthropicOpts::default());
    assert_eq!(a, b, "transform must be deterministic (cache stability)");
}

#[test]
fn tools_stubbed_and_schema_stripped() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": big_system(),
        "tools": [{
            "name": "edit_file",
            "description": "very long tool description ".repeat(50),
            "input_schema": {
                "type": "object",
                "description": "annotation",
                "properties": { "path": { "type": "string", "description": "p" } },
                "required": ["path"]
            },
            "cache_control": { "type": "ephemeral" }
        }],
        "messages": [ { "role": "user", "content": "go" } ]
    });
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(info.compressed);
    let tool = &root["tools"][0];
    assert!(tool["description"]
        .as_str()
        .unwrap()
        .contains("## Tool: edit_file"));
    assert!(tool["input_schema"].get("description").is_none());
    assert_eq!(tool["input_schema"]["required"], json!(["path"]));
    assert!(tool.get("cache_control").is_none(), "tool anchor relocated");
}

/// Dense filler text large enough to clear the live-block gate.
fn big_text() -> String {
    "log line with detail /var/run/app.sock port 8080 CODE_X 12345 ".repeat(300)
}

#[test]
fn images_large_reminder_block() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "small",
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": format!("<system-reminder>{}</system-reminder>", big_text()) },
                { "type": "text", "text": "the actual question" }
            ]
        }]
    });
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(info.compressed);
    assert!(info.reminder_imgs >= 1);
    let content = root["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image", "reminder replaced by image");
    // The real question text survives untouched.
    assert!(content.iter().any(|b| b["text"] == "the actual question"));
}

#[test]
fn images_large_tool_result_with_factsheet() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "small",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tu_1",
                "content": big_text(),
                "cache_control": { "type": "ephemeral" }
            }]
        }]
    });
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(info.compressed);
    assert!(info.tool_result_imgs >= 1);
    let tr = &root["messages"][0]["content"][0];
    assert!(
        tr.get("cache_control").is_none(),
        "anchor relocated onto image"
    );
    let inner = tr["content"].as_array().unwrap();
    assert_eq!(inner[0]["type"], "image");
    // fact-sheet text block rides alongside the image (paths/ids survive OCR).
    assert!(inner
        .iter()
        .any(|b| b["type"] == "text" && b["text"].as_str().unwrap().contains("/var/run/app.sock")));
}

#[test]
fn skips_error_tool_result() {
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "small",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tu_1",
                "is_error": true,
                "content": big_text()
            }]
        }]
    });
    let before = root.clone();
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(!info.compressed, "error tool_results must not be imaged");
    assert_eq!(root, before);
}

#[test]
fn pages_oversized_tool_result() {
    // Force a tiny image cap so a modest result trips the paging path.
    let opts = AnthropicOpts {
        max_images_per_tool_result: 1,
        max_height_px: 80, // ~9 rows/page → many pages before truncation
        ..AnthropicOpts::default()
    };
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "small",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tu_1",
                "content": big_text()
            }]
        }]
    });
    let info = transform(&mut root, &opts);
    assert!(info.compressed);
    assert_eq!(info.truncated_tool_results, 1);
    assert!(info.omitted_chars > 0);
}

/// A conversation with `pairs` closed assistant(tool_use)/user(tool_result) turns.
fn convo(pairs: usize) -> serde_json::Value {
    let mut msgs = vec![json!({ "role": "user", "content": "start the task" })];
    for i in 0..pairs {
        msgs.push(json!({
            "role": "assistant",
            "content": [
                { "type": "text", "text": format!("step {i}") },
                { "type": "tool_use", "id": format!("tu_{i}"), "name": "run", "input": { "cmd": format!("do {i}") } }
            ]
        }));
        msgs.push(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": format!("tu_{i}"),
                "content": format!("output /path/file{i}.rs {}", "detail ".repeat(60))
            }]
        }));
    }
    json!({ "model": "claude-fable-5", "system": "small", "messages": msgs })
}

fn history_opts() -> AnthropicOpts {
    AnthropicOpts {
        compress_history: true,
        // Isolate the history pass so tail tool_results don't also image.
        compress_tool_results: false,
        compress_reminders: false,
        ..AnthropicOpts::default()
    }
}

#[test]
fn collapses_closed_history_prefix() {
    let mut root = convo(18); // 37 messages
    let before_len = root["messages"].as_array().unwrap().len();
    let info = transform(&mut root, &history_opts());
    assert!(info.compressed, "reason={}", info.reason);
    assert_eq!(info.collapsed_turns, 20, "snapped to the 20-message grid");
    assert!(info.collapsed_images >= 1);

    let msgs = root["messages"].as_array().unwrap();
    assert!(msgs.len() < before_len, "prefix collapsed");
    // First user message is protected (untouched).
    assert_eq!(msgs[0]["content"], "start the task");
    // Synthetic history message: intro text, then an image.
    let syn = msgs[1]["content"].as_array().unwrap();
    assert!(syn[0]["text"]
        .as_str()
        .unwrap()
        .contains("EARLIER conversation"));
    assert!(syn.iter().any(|b| b["type"] == "image"));
    assert!(syn.last().unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("live request follows"));
}

#[test]
fn history_render_is_cache_stable() {
    let mut a = convo(18);
    let mut b = convo(18);
    transform(&mut a, &history_opts());
    transform(&mut b, &history_opts());
    assert_eq!(a, b, "same conversation must collapse to identical bytes");
}

#[test]
fn short_history_not_collapsed() {
    // Below min_collapse_prefix_messages (20) → left as text.
    let mut root = convo(5); // 11 messages
    let before = root.clone();
    let info = transform(&mut root, &history_opts());
    assert!(!info.compressed);
    assert_eq!(root, before);
}

#[test]
fn total_image_budget_is_enforced() {
    // Many large tool_results but a tiny total budget: imaging stops at the
    // cap so the request can't exceed Anthropic's per-request image limit.
    let mut msgs = vec![json!({ "role": "user", "content": "start" })];
    for i in 0..10 {
        msgs.push(json!({
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": format!("tu_{i}"), "content": big_text() }]
        }));
    }
    let mut root = json!({ "model": "claude-fable-5", "system": "small", "messages": msgs });
    let opts = AnthropicOpts {
        max_total_images: 3,
        ..AnthropicOpts::default()
    };
    let info = transform(&mut root, &opts);
    assert!(info.compressed);
    assert!(
        info.image_count <= 3,
        "aggregate image budget exceeded: {}",
        info.image_count
    );
}

#[test]
fn tool_result_carrying_an_image_is_left_alone() {
    // A screenshot + long log in one tool_result: imaging the log would drop
    // the screenshot, so the whole result must pass through untouched.
    let mut root = json!({
        "model": "claude-fable-5",
        "system": "small",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tu_1",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } },
                    { "type": "text", "text": big_text() }
                ]
            }]
        }]
    });
    let before = root.clone();
    let info = transform(&mut root, &AnthropicOpts::default());
    assert!(
        !info.compressed,
        "tool_result with an image must not be imaged"
    );
    assert_eq!(root, before);
}

/// True unless a tool_result references a tool_use id not present as a real
/// block earlier in the message list (an orphan the API rejects).
fn no_orphaned_tool_results(root: &serde_json::Value) -> bool {
    let mut seen = std::collections::HashSet::new();
    for m in root["messages"].as_array().unwrap() {
        let Some(blocks) = m.get("content").and_then(|c| c.as_array()) else {
            continue;
        };
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("tool_use") => {
                    if let Some(id) = b.get("id").and_then(|i| i.as_str()) {
                        seen.insert(id.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = b.get("tool_use_id").and_then(|i| i.as_str()) {
                        if !seen.contains(id) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    true
}

#[test]
fn history_snap_never_orphans_a_tool_result() {
    // A text-only assistant turn right after the first user message shifts
    // parity so the chunk-grid line (61) lands mid-pair on an OPEN tool_use;
    // the snap must back off to the nearest closed boundary (60) instead of
    // orphaning tu_29's tool_result into the tail.
    let mut msgs = vec![json!({ "role": "user", "content": "start" })];
    msgs.push(json!({ "role": "assistant", "content": [{ "type": "text", "text": "thinking out loud" }] }));
    for i in 0..40 {
        msgs.push(json!({
            "role": "assistant",
            "content": [
                { "type": "text", "text": format!("step {i}") },
                { "type": "tool_use", "id": format!("tu_{i}"), "name": "run", "input": { "cmd": format!("do {i}") } }
            ]
        }));
        msgs.push(json!({
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": format!("tu_{i}"), "content": format!("out /p/f{i}.rs {}", "detail ".repeat(60)) }]
        }));
    }
    let mut root = json!({ "model": "claude-fable-5", "system": "small", "messages": msgs });
    let info = transform(&mut root, &history_opts());
    assert!(info.compressed, "reason={}", info.reason);
    assert!(
        no_orphaned_tool_results(&root),
        "history collapse orphaned a tool_result"
    );
}

#[test]
fn open_tool_call_not_crossed() {
    // Last prefix turn leaves a tool_use unmatched (open); the boundary must
    // stop before it so no tool call is orphaned into the image.
    let mut root = convo(18);
    // Drop the tool_result of an early pair to open a call at message 4.
    root["messages"][4] = json!({ "role": "user", "content": "no tool result here" });
    let info = transform(&mut root, &history_opts());
    // With an open call at msg 3 (tu_1) never closed, the closed boundary
    // can't advance past msg 2, so the 20-message grid step never fills.
    assert!(!info.compressed || info.collapsed_turns == 0);
}
