pub mod common;
pub mod history;
pub mod reminders;
pub mod slab;
pub mod tool_results;

#[cfg(test)]
mod tests;

use crate::transform::info::TransformInfo;
use serde_json::Value;

pub use common::AnthropicOpts;

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
        .saturating_sub(common::count_existing_images(root));
    let slab_reason = slab::apply_slab(root, opts, &mut info, &mut budget);
    // History runs BEFORE the live-region passes: it serializes the OLD message
    // prefix to text, so tool_result imaging must not have already replaced that
    // content with `[image]` placeholders. Reminders/tool_results then image only
    // what survives (the protected first message + the live tail).
    if opts.compress_history {
        history::apply_history(root, opts, &mut info, &mut budget);
    }
    if opts.compress_reminders {
        reminders::apply_reminders(root, opts, &mut info, &mut budget);
    }
    if opts.compress_tool_results {
        tool_results::apply_tool_results(root, opts, &mut info, &mut budget);
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
