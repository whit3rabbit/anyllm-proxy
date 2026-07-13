use super::common::{render_live_block, AnthropicOpts};
use crate::transform::info::TransformInfo;
use serde_json::Value;

/// Image large `<system-reminder>` text blocks in the first user message. These
/// are per-turn injected context (env, hints) that Claude Code ships as separate
/// text blocks; the big ones are pure token cost the model rarely needs to quote.
pub(crate) fn apply_reminders(
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
