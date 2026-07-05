//! Per-request transform telemetry. Subset of pxpipe's `TransformInfo` — only
//! the fields the proxy actually surfaces (tracing / metrics). No dashboard.

/// Result of a transform attempt. `compressed == false` means the body was left
/// untouched; `reason` says why (or `"applied"` when it was compressed).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransformInfo {
    pub compressed: bool,
    /// Machine-readable outcome: "applied" | "below_min_chars" | "not_profitable"
    /// | "no_slab" | "parse_error".
    pub reason: &'static str,
    /// Source chars replaced by image blocks (the slab that was imaged).
    pub compressed_chars: usize,
    /// PNG image blocks emitted.
    pub image_count: usize,
    /// Total PNG bytes emitted.
    pub image_bytes: usize,
    /// Σ width×height across images (pairs with upstream cache tokens later).
    pub image_pixels: usize,
    /// Codepoints missing from the atlas, rendered as blank cells.
    pub dropped_chars: usize,
    /// True when a caller `cache_control` breakpoint was relocated onto the image.
    pub relocated_cache_anchor: bool,
    /// Images emitted from compressing `<system-reminder>` blocks.
    pub reminder_imgs: usize,
    /// Images emitted from compressing `tool_result` content.
    pub tool_result_imgs: usize,
    /// tool_results whose text exceeded the per-result image budget and was truncated.
    pub truncated_tool_results: usize,
    /// Source chars elided by paging/truncation across all tool_results.
    pub omitted_chars: usize,
    /// Messages collapsed into the synthetic history image message.
    pub collapsed_turns: usize,
    /// Source chars serialized into the history image(s).
    pub collapsed_chars: usize,
    /// PNG image blocks emitted for the collapsed history (also in `image_count`).
    pub collapsed_images: usize,
}

impl TransformInfo {
    pub fn skipped(reason: &'static str) -> Self {
        Self {
            reason,
            ..Default::default()
        }
    }
}
