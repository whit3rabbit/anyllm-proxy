use crate::render::{render_text, RenderOpts};
use crate::transform::factsheet;
use crate::transform::gate;
use base64::Engine;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug)]
pub struct AnthropicOpts {
    pub cols: usize,
    pub max_height_px: usize,
    pub chars_per_token: f64,
    /// Skip entirely below this many slab chars (per-image cost dominates).
    pub min_compress_chars: usize,
    /// Move tool descriptions/schema annotations into the imaged Tool Reference.
    pub compress_tools: bool,
    /// Image large `<system-reminder>` text blocks in the first user message.
    pub compress_reminders: bool,
    /// Image large `tool_result` text content across all user messages.
    pub compress_tool_results: bool,
    /// Per-block char floor for reminder/tool_result imaging.
    pub min_live_block_chars: usize,
    /// Cap on images per tool_result; source is truncated (with a paging marker)
    /// above this so one giant result can't blow Anthropic's 100-image/request cap.
    pub max_images_per_tool_result: usize,
    /// Aggregate ceiling on TOTAL image blocks in the outgoing request
    /// (client-supplied + every pxpipe pass combined). Anthropic rejects requests
    /// past ~100 images, so imaging stops once this budget is exhausted and the
    /// remaining regions pass through as text. The per-tool_result cap only bounds
    /// one result; this bounds the sum.
    pub max_total_images: usize,
    /// Collapse the OLD closed-tool-call conversation prefix into history image(s),
    /// keeping the recent tail as text. Default OFF — highest cache-stability risk
    /// (see `apply_history`); gate on live validation before enabling by default.
    pub compress_history: bool,
    /// Trailing messages kept as live text (never collapsed).
    pub keep_tail_messages: usize,
    /// Minimum collapsible messages; below this the cache-amortization math doesn't pay.
    pub min_collapse_prefix_messages: usize,
    /// Snap the collapse boundary to this message-grid so the rendered history PNG
    /// stays byte-identical across turns and keeps hitting the prompt cache.
    pub history_collapse_chunk: usize,
}

impl Default for AnthropicOpts {
    fn default() -> Self {
        Self {
            cols: crate::render::RenderOpts::default().cols,
            max_height_px: crate::render::RenderOpts::default().max_height_px,
            // Slab is dense; 2.0 is conservative vs the real ~1.9 (see pxpipe
            // SLAB_CHARS_PER_TOKEN) so the gate biases toward pass-through.
            chars_per_token: 2.0,
            min_compress_chars: 6_000,
            compress_tools: true,
            compress_reminders: true,
            compress_tool_results: true,
            min_live_block_chars: 2_000,
            max_images_per_tool_result: 10,
            // Leave headroom under Anthropic's 100-image/request cap for a few
            // client-supplied images the counter may miss on unusual shapes.
            max_total_images: 95,
            compress_history: false,
            keep_tail_messages: 8,
            min_collapse_prefix_messages: 20,
            history_collapse_chunk: 20,
        }
    }
}

/// Neutral framing header co-rendered into the slab image. Deliberately avoids
/// "system prompt"/"authoritative" wording — pxpipe found that phrasing trips
/// Anthropic's reasoning_extraction refusal (reads as a replayed/extracted
/// prompt). First-party, matter-of-fact framing keeps the model reading it as
/// this session's own reference material.
pub(crate) const SLAB_HEADER: &str =
    "Reference context for this session, rendered as an image. Read it as text.\n\n";

/// Chars-per-token for the live-region gate (reminders, tool_results). Higher
/// than the slab's 2.0 because that content is prose/log, not dense config —
/// pxpipe uses 4 here, which is conservative (biases toward pass-through).
pub(crate) const LIVE_CHARS_PER_TOKEN: f64 = 4.0;

/// Banner text bracketing the collapsed-history image(s). Constant (byte-stable).
pub(crate) const HISTORY_INTRO: &str = "Transcript of EARLIER conversation turns, rendered as images below. \
Attribute each turn strictly by its <user>/<assistant> tag. This is PAST context, not the live request.\n";
pub(crate) const HISTORY_OUTRO: &str =
    "\n[End of earlier transcript. The live request follows in the messages below.]";

/// Concat the text of a system field (string or block array) and return
/// `(text, last_cache_control)`. The cache_control is the caller's prefix
/// breakpoint we will relocate onto the image.
pub(crate) fn read_system(system: Option<&Value>) -> (String, Option<Value>) {
    match system {
        Some(Value::String(s)) => (s.clone(), None),
        Some(Value::Array(blocks)) => {
            let mut text = String::new();
            let mut cc = None;
            for b in blocks {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(t);
                }
                if let Some(c) = b.get("cache_control") {
                    cc = Some(c.clone()); // last one wins (latest in prefix)
                }
            }
            (text, cc)
        }
        _ => (String::new(), None),
    }
}

/// Render one tool's full doc (prose + compact schema) for the imaged reference.
pub(crate) fn render_tool_doc(tool: &Value) -> String {
    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let desc = tool
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let schema = tool
        .get("input_schema")
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .unwrap_or_default();
    format!("## Tool: {name}\n{desc}\n```json\n{schema}\n```\n")
}

/// Build a base64 image content block from PNG bytes.
pub(crate) fn image_block(png: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    json!({
        "type": "image",
        "source": { "type": "base64", "media_type": "image/png", "data": data }
    })
}

/// Count image blocks already present in the request (client-supplied, incl.
/// images nested inside `tool_result` content), so the added-image budget is
/// measured against the true outgoing total under Anthropic's ~100-image cap.
pub(crate) fn count_existing_images(root: &Value) -> usize {
    let Some(msgs) = root.get("messages").and_then(|m| m.as_array()) else {
        return 0;
    };
    let is_image = |b: &Value| b.get("type").and_then(|t| t.as_str()) == Some("image");
    let mut n = 0;
    for m in msgs {
        let Some(blocks) = m.get("content").and_then(|c| c.as_array()) else {
            continue;
        };
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("image") => n += 1,
                Some("tool_result") => {
                    if let Some(inner) = b.get("content").and_then(|c| c.as_array()) {
                        n += inner.iter().filter(|x| is_image(x)).count();
                    }
                }
                _ => {}
            }
        }
    }
    n
}

/// Prepend `images` to the first user message's content. Converts string
/// content to a text block first; inserts a fresh user message if none exists.
pub(crate) fn prepend_images_to_first_user(obj: &mut Map<String, Value>, images: &mut Vec<Value>) {
    let msgs = obj
        .entry("messages")
        .or_insert_with(|| Value::Array(vec![]));
    let Some(arr) = msgs.as_array_mut() else {
        return;
    };
    let first_user = arr
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

    match first_user {
        Some(idx) => {
            let content = arr[idx].as_object_mut().and_then(|m| m.get_mut("content"));
            match content {
                Some(Value::String(s)) => {
                    let text = std::mem::take(s);
                    let mut blocks = vec![json!({ "type": "text", "text": text })];
                    let mut new = std::mem::take(images);
                    new.append(&mut blocks);
                    arr[idx]
                        .as_object_mut()
                        .unwrap()
                        .insert("content".into(), Value::Array(new));
                }
                Some(Value::Array(existing)) => {
                    let mut new = std::mem::take(images);
                    new.append(existing);
                    *existing = new;
                }
                _ => {
                    arr[idx]
                        .as_object_mut()
                        .unwrap()
                        .insert("content".into(), Value::Array(std::mem::take(images)));
                }
            }
        }
        None => {
            arr.insert(
                0,
                json!({ "role": "user", "content": std::mem::take(images) }),
            );
        }
    }
}

/// Rendered-live-block accumulators, folded into `TransformInfo` by the caller.
pub(crate) struct LiveRender {
    pub(crate) blocks: Vec<Value>,
    pub(crate) bytes: usize,
    pub(crate) pixels: usize,
    pub(crate) dropped: usize,
    pub(crate) img_count: usize,
}

/// Render `text` to image blocks for a live region (reminder / tool_result),
/// gated on profitability AND the remaining `budget` of new images. Moves `cc`
/// (the block's cache_control, if any) onto the last image and optionally
/// appends a verbatim fact-sheet text block. Returns `None` (leave the block as
/// text) when empty, not profitable, or over budget.
pub(crate) fn render_live_block(
    text: &str,
    opts: &AnthropicOpts,
    cc: Option<Value>,
    append_factsheet: bool,
    budget: usize,
) -> Option<LiveRender> {
    let images = render_text(
        text,
        RenderOpts {
            cols: opts.cols,
            max_height_px: opts.max_height_px,
        },
    );
    let chars = text.chars().count();
    if images.len() > budget {
        return None;
    }
    if !gate::is_profitable(&images, chars, LIVE_CHARS_PER_TOKEN) {
        return None;
    }
    let mut blocks: Vec<Value> = images.iter().map(|im| image_block(&im.png)).collect();
    if let (Some(cc), Some(last)) = (cc, blocks.last_mut()) {
        if let Some(o) = last.as_object_mut() {
            o.insert("cache_control".into(), cc);
        }
    }
    if append_factsheet {
        let fact = factsheet::fact_sheet_text(text);
        if !fact.is_empty() {
            blocks.push(json!({ "type": "text", "text": fact }));
        }
    }
    Some(LiveRender {
        blocks,
        bytes: images.iter().map(|im| im.png.len()).sum(),
        pixels: images
            .iter()
            .map(|im| im.width as usize * im.height as usize)
            .sum(),
        dropped: images.iter().map(|im| im.dropped).sum(),
        img_count: images.len(),
    })
}

/// Flatten a tool_result `content` (string, or an array of text/other blocks)
/// to a single text string for rendering.
pub(crate) fn tool_result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut out = String::new();
            for b in blocks {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// True when a tool_result `content` array already holds an image block. Imaging
/// such a result would drop that image (only text sub-blocks are flattened), so
/// the caller skips it.
pub(crate) fn tool_result_has_image(content: Option<&Value>) -> bool {
    matches!(content, Some(Value::Array(blocks))
        if blocks
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("image")))
}

/// Truncate `text` so it renders to at most `max_images` pages, keeping a 60/40
/// head/tail split with a paging marker in the middle. Returns the (possibly
/// truncated) text and the count of chars elided. Renders once to measure — the
/// oversized path is rare, so the extra render is acceptable.
pub(crate) fn truncate_for_budget(
    text: &str,
    max_images: usize,
    opts: &AnthropicOpts,
) -> (String, usize) {
    let render_opts = RenderOpts {
        cols: opts.cols,
        max_height_px: opts.max_height_px,
    };
    let full = render_text(text, render_opts);
    let cap = max_images.max(1);
    if full.len() <= cap {
        return (text.to_string(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    // Start from the linear estimate, then shrink and RE-RENDER until it truly
    // fits `cap` pages — the paging marker and heavy line-wrapping can push a
    // linear estimate over, so the cap must be verified, not assumed.
    let mut ratio = cap as f64 / full.len() as f64;
    loop {
        let keep = (((chars.len() as f64) * ratio) as usize).min(chars.len());
        let head_len = keep * 6 / 10;
        let tail_len = keep - head_len;
        let head: String = chars[..head_len].iter().collect();
        let tail: String = chars[chars.len() - tail_len..].iter().collect();
        let omitted = chars.len() - head_len - tail_len;
        let out = format!("{head}\n\n[... {omitted} chars omitted for length ...]\n\n{tail}");
        if render_text(&out, render_opts).len() <= cap || ratio < 0.05 {
            return (out, omitted);
        }
        ratio *= 0.8;
    }
}
