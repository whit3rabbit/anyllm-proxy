use super::common::{
    render_live_block, tool_result_has_image, tool_result_text, truncate_for_budget, AnthropicOpts,
};
use crate::transform::info::TransformInfo;
use serde_json::Value;

/// Image large `tool_result` text content across every user message. tool_result
/// output (find trees, file dumps, logs) is the bulk of agentic input. Skips
/// `is_error` results (Anthropic rejects images inside those) and pages oversized
/// content down to the image cap.
pub(crate) fn apply_tool_results(
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
