//! Anthropic Messages request transform (static-slab imaging). Port of the
//! system-slab + tool-doc path of pxpipe's `transform.ts`.
//!
//! **Value-based on purpose.** The proxy's typed `anthropic::ContentBlock`/`Tool`
//! do not model per-block `cache_control` and have no flatten catch-all, so a
//! typed round-trip would silently drop every cache breakpoint and unmodeled
//! block. We read/mutate the raw `serde_json::Value` tree and touch only the
//! system field, the tools array, and the first user message — every other byte
//! passes through untouched. This is why we cannot reuse the proxy's
//! `patch_repaired_body` (which *fails open* on cache_control to preserve it in
//! place; we need to *relocate* it onto the image).

use base64::Engine;
use serde_json::{json, Map, Value};

use super::info::TransformInfo;
use super::{factsheet, gate, schema_strip};
use crate::render::{render_text, RenderOpts};

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
const SLAB_HEADER: &str =
    "Reference context for this session, rendered as an image. Read it as text.\n\n";

/// Concat the text of a system field (string or block array) and return
/// `(text, last_cache_control)`. The cache_control is the caller's prefix
/// breakpoint we will relocate onto the image.
fn read_system(system: Option<&Value>) -> (String, Option<Value>) {
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
fn render_tool_doc(tool: &Value) -> String {
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
fn image_block(png: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    json!({
        "type": "image",
        "source": { "type": "base64", "media_type": "image/png", "data": data }
    })
}

/// Chars-per-token for the live-region gate (reminders, tool_results). Higher
/// than the slab's 2.0 because that content is prose/log, not dense config —
/// pxpipe uses 4 here, which is conservative (biases toward pass-through).
const LIVE_CHARS_PER_TOKEN: f64 = 4.0;

/// Transform `root` (a parsed Anthropic Messages body) in place. Returns info;
/// on any full skip `root` is left byte-identical to the input. Three
/// independent passes — static slab, `<system-reminder>` blocks, and
/// `tool_result` content — so a request below the slab floor can still get its
/// live regions imaged (and vice-versa).
pub fn transform(root: &mut Value, opts: &AnthropicOpts) -> TransformInfo {
    if !root.is_object() {
        return TransformInfo::skipped("parse_error");
    }
    let mut info = TransformInfo::default();
    // NEW-image budget: total ceiling minus images the client already sent, so
    // the combined passes never push the request past Anthropic's ~100-image cap.
    let mut budget = opts
        .max_total_images
        .saturating_sub(count_existing_images(root));
    let slab_reason = apply_slab(root, opts, &mut info, &mut budget);
    // History runs BEFORE the live-region passes: it serializes the OLD message
    // prefix to text, so tool_result imaging must not have already replaced that
    // content with `[image]` placeholders. Reminders/tool_results then image only
    // what survives (the protected first message + the live tail).
    if opts.compress_history {
        apply_history(root, opts, &mut info, &mut budget);
    }
    if opts.compress_reminders {
        apply_reminders(root, opts, &mut info, &mut budget);
    }
    if opts.compress_tool_results {
        apply_tool_results(root, opts, &mut info, &mut budget);
    }
    info.compressed = info.image_count > 0;
    // "applied" when anything imaged; otherwise report why the slab (the primary
    // path) declined.
    info.reason = if info.compressed {
        "applied"
    } else {
        slab_reason
    };
    info
}

/// Count image blocks already present in the request (client-supplied, incl.
/// images nested inside `tool_result` content), so the added-image budget is
/// measured against the true outgoing total under Anthropic's ~100-image cap.
fn count_existing_images(root: &Value) -> usize {
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

/// Render `text` to image blocks for a live region (reminder / tool_result),
/// gated on profitability AND the remaining `budget` of new images. Moves `cc`
/// (the block's cache_control, if any) onto the last image and optionally
/// appends a verbatim fact-sheet text block. Returns `None` (leave the block as
/// text) when empty, not profitable, or over budget.
fn render_live_block(
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

/// Rendered-live-block accumulators, folded into `TransformInfo` by the caller.
struct LiveRender {
    blocks: Vec<Value>,
    bytes: usize,
    pixels: usize,
    dropped: usize,
    img_count: usize,
}

/// Static system+tools slab pass. Returns the skip reason (or "applied"); only
/// mutates `root` / fills `info` when profitable.
fn apply_slab(
    root: &mut Value,
    opts: &AnthropicOpts,
    info: &mut TransformInfo,
    budget: &mut usize,
) -> &'static str {
    let (system_text, system_cc) = read_system(root.get("system"));

    // Capture a tool cache_control as a fallback anchor (tools sit before system
    // in the cache prefix, so system's wins when both exist).
    let tools_present = opts.compress_tools
        && root
            .get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|a| !a.is_empty());
    let mut tool_cc = None;
    let mut tool_ref = String::new();
    if tools_present {
        if let Some(arr) = root.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                tool_ref.push_str(&render_tool_doc(t));
                if let Some(c) = t.get("cache_control") {
                    tool_cc = Some(c.clone());
                }
            }
        }
    }

    // Assemble the slab: header + system + tool reference.
    let mut slab = String::with_capacity(system_text.len() + tool_ref.len() + 64);
    slab.push_str(SLAB_HEADER);
    slab.push_str(&system_text);
    if !tool_ref.is_empty() {
        slab.push_str("\n\n# Tool Reference\n");
        slab.push_str(&tool_ref);
    }

    let slab_chars = slab.chars().count();
    // Nothing meaningful to image (no system text and no tools).
    if system_text.trim().is_empty() && tool_ref.is_empty() {
        return "no_slab";
    }
    if slab_chars < opts.min_compress_chars {
        return "below_min_chars";
    }

    let images = render_text(
        &slab,
        RenderOpts {
            cols: opts.cols,
            max_height_px: opts.max_height_px,
        },
    );
    if !gate::is_profitable(&images, slab_chars, opts.chars_per_token) {
        return "not_profitable";
    }
    if images.len() > *budget {
        return "image_budget";
    }

    // ---- commit: mutate root ------------------------------------------------
    let anchor = system_cc.or(tool_cc); // relocate this onto the last image
    let fact = factsheet::fact_sheet_text(&slab);

    let mut image_blocks: Vec<Value> = images.iter().map(|im| image_block(&im.png)).collect();
    let image_bytes: usize = images.iter().map(|im| im.png.len()).sum();
    let image_pixels: usize = images
        .iter()
        .map(|im| im.width as usize * im.height as usize)
        .sum();
    let dropped: usize = images.iter().map(|im| im.dropped).sum();

    // Relocate the caller's cache breakpoint onto the LAST image so the whole
    // imaged prefix caches as one stable segment. pxpipe never *adds* a marker.
    let relocated = if let (Some(cc), Some(last)) = (anchor, image_blocks.last_mut()) {
        if let Some(obj) = last.as_object_mut() {
            obj.insert("cache_control".into(), cc);
        }
        true
    } else {
        false
    };

    let obj = root.as_object_mut().expect("checked is_object above");

    // 1. system -> short pointer (+ factsheet). Removes original cache_control
    //    (relocated onto the image) since we're replacing the whole field.
    let mut pointer = String::from(
        "[Your reference context for this session is provided as an image in the first user message. Read it there.]",
    );
    if !fact.is_empty() {
        pointer.push('\n');
        pointer.push_str(&fact);
    }
    obj.insert("system".into(), Value::String(pointer));

    // 2. tools -> stub description + annotation-stripped schema, drop the
    //    relocated cache_control. Keep name + structural schema so Anthropic's
    //    tool-use validator still accepts calls.
    if tools_present {
        if let Some(arr) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
            for t in arr.iter_mut() {
                let Some(tm) = t.as_object_mut() else {
                    continue;
                };
                let name = tm
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                tm.insert(
                    "description".into(),
                    Value::String(format!("See \"## Tool: {name}\" in the reference image.")),
                );
                if let Some(schema) = tm.get("input_schema") {
                    let stripped = schema_strip::strip(schema);
                    if schema_strip::has_structure(&stripped) {
                        tm.insert("input_schema".into(), stripped);
                    }
                    // else keep original: a bare {type:object} stub causes 400s.
                }
                tm.remove("cache_control");
            }
        }
    }

    // 3. Prepend image blocks to the first user message (system rejects images).
    prepend_images_to_first_user(obj, &mut image_blocks);

    info.compressed_chars += slab.len();
    info.image_count += images.len();
    info.image_bytes += image_bytes;
    info.image_pixels += image_pixels;
    info.dropped_chars += dropped;
    info.relocated_cache_anchor = relocated;
    *budget -= images.len();
    "applied"
}

/// Prepend `images` to the first user message's content. Converts string
/// content to a text block first; inserts a fresh user message if none exists.
fn prepend_images_to_first_user(obj: &mut Map<String, Value>, images: &mut Vec<Value>) {
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

/// Image large `<system-reminder>` text blocks in the first user message. These
/// are per-turn injected context (env, hints) that Claude Code ships as separate
/// text blocks; the big ones are pure token cost the model rarely needs to quote.
fn apply_reminders(
    root: &mut Value,
    opts: &AnthropicOpts,
    info: &mut TransformInfo,
    budget: &mut usize,
) {
    let Some(arr) = root.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    let Some(msg) = arr
        .iter_mut()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    else {
        return;
    };
    let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
        return; // string content: reminders arrive only as array blocks
    };

    let mut out: Vec<Value> = Vec::with_capacity(content.len());
    for block in std::mem::take(content) {
        let is_reminder_text = block.get("type").and_then(|t| t.as_str()) == Some("text")
            && block.get("text").and_then(|t| t.as_str()).is_some_and(|t| {
                t.contains("<system-reminder>") && t.len() >= opts.min_live_block_chars
            });
        if !is_reminder_text {
            out.push(block);
            continue;
        }
        let text = block
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        let cc = block.get("cache_control").cloned();
        match render_live_block(&text, opts, cc, false, *budget) {
            Some(r) => {
                *budget -= r.img_count;
                info.reminder_imgs += r.img_count;
                info.image_count += r.img_count;
                info.image_bytes += r.bytes;
                info.image_pixels += r.pixels;
                info.dropped_chars += r.dropped;
                info.compressed_chars += text.len();
                out.extend(r.blocks);
            }
            None => out.push(block),
        }
    }
    *content = out;
}

/// Image large `tool_result` text content across every user message. tool_result
/// output (find trees, file dumps, logs) is the bulk of agentic input. Skips
/// `is_error` results (Anthropic rejects images inside those) and pages oversized
/// content down to the image cap.
fn apply_tool_results(
    root: &mut Value,
    opts: &AnthropicOpts,
    info: &mut TransformInfo,
    budget: &mut usize,
) {
    let Some(msgs) = root.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for msg in msgs.iter_mut() {
        if *budget == 0 {
            break;
        }
        let Some(blocks) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        for block in blocks.iter_mut() {
            let Some(bm) = block.as_object_mut() else {
                continue;
            };
            if bm.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            if bm.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                continue; // images forbidden in error tool_results
            }
            // Don't clobber a tool_result that already carries an image block
            // (e.g. a screenshot the tool returned) — tool_result_text only reads
            // the text sub-blocks, so imaging here would silently drop that image.
            if tool_result_has_image(bm.get("content")) {
                continue;
            }
            let text = tool_result_text(bm.get("content"));
            if text.len() < opts.min_live_block_chars {
                continue;
            }
            // Cap each result at the per-result limit AND whatever total budget
            // is left, so the aggregate can't exceed max_total_images.
            let per_result = opts.max_images_per_tool_result.min(*budget);
            if per_result == 0 {
                break;
            }
            let (rendered, omitted) = truncate_for_budget(&text, per_result, opts);
            let cc = bm.get("cache_control").cloned();
            if let Some(r) = render_live_block(&rendered, opts, cc, true, *budget) {
                *budget -= r.img_count;
                if omitted > 0 {
                    info.truncated_tool_results += 1;
                    info.omitted_chars += omitted;
                }
                info.tool_result_imgs += r.img_count;
                info.image_count += r.img_count;
                info.image_bytes += r.bytes;
                info.image_pixels += r.pixels;
                info.dropped_chars += r.dropped;
                info.compressed_chars += text.len();
                bm.remove("cache_control"); // relocated onto the last image
                bm.insert("content".into(), Value::Array(r.blocks));
            }
        }
    }
}

/// True when a tool_result `content` array already holds an image block. Imaging
/// such a result would drop that image (only text sub-blocks are flattened), so
/// the caller skips it.
fn tool_result_has_image(content: Option<&Value>) -> bool {
    matches!(content, Some(Value::Array(blocks))
        if blocks
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("image")))
}

/// Flatten a tool_result `content` (string, or an array of text/other blocks)
/// to a single text string for rendering.
fn tool_result_text(content: Option<&Value>) -> String {
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

/// Truncate `text` so it renders to at most `max_images` pages, keeping a 60/40
/// head/tail split with a paging marker in the middle. Returns the (possibly
/// truncated) text and the count of chars elided. Renders once to measure — the
/// oversized path is rare, so the extra render is acceptable.
fn truncate_for_budget(text: &str, max_images: usize, opts: &AnthropicOpts) -> (String, usize) {
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

/// Banner text bracketing the collapsed-history image(s). Constant (byte-stable).
const HISTORY_INTRO: &str = "Transcript of EARLIER conversation turns, rendered as images below. \
Attribute each turn strictly by its <user>/<assistant> tag. This is PAST context, not the live request.\n";
const HISTORY_OUTRO: &str =
    "\n[End of earlier transcript. The live request follows in the messages below.]";

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
fn apply_history(
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

#[cfg(test)]
mod tests {
    use super::*;

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
            .any(|b| b["type"] == "text"
                && b["text"].as_str().unwrap().contains("/var/run/app.sock")));
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
    fn convo(pairs: usize) -> Value {
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
    fn no_orphaned_tool_results(root: &Value) -> bool {
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
}
