//! Break-even gate. Port of the profitability check in pxpipe's `transform.ts`.
//!
//! We render the slab first, then compare the *actual* image-token cost against
//! the text-token cost of the same content. Rendering first (instead of
//! pxpipe's pre-render estimate) trades a wasted render on unprofitable requests
//! — cheap, the slab is not on any hot loop — for exactness: the gate can never
//! disagree with what we'd actually ship.

use crate::render::RenderedImage;

/// Anthropic image billing: `tokens ≈ w·h / 750`, with a 10% safety margin so
/// borderline requests bias toward pass-through (never toward a net-loss image).
/// <https://docs.anthropic.com/en/docs/build-with-claude/vision#image-tokens>
pub const ANTHROPIC_PIXELS_PER_TOKEN: f64 = 750.0;
pub const IMAGE_COST_SAFETY_MARGIN: f64 = 1.10;

/// Total image-token cost of the rendered pages.
pub fn image_tokens(images: &[RenderedImage]) -> u64 {
    images
        .iter()
        .map(|im| {
            let px = im.width as f64 * im.height as f64;
            (px / ANTHROPIC_PIXELS_PER_TOKEN * IMAGE_COST_SAFETY_MARGIN).ceil() as u64
        })
        .sum()
}

/// Text-token cost of `char_count` at the given chars-per-token assumption.
pub fn text_tokens(char_count: usize, chars_per_token: f64) -> u64 {
    (char_count as f64 / chars_per_token).ceil() as u64
}

/// True when imaging `images` (standing in for `char_count` chars of text) costs
/// fewer tokens than leaving it as text. Cold-start form (no warm-cache burn
/// terms yet — those arrive with the history phase).
pub fn is_profitable(images: &[RenderedImage], char_count: usize, chars_per_token: f64) -> bool {
    !images.is_empty() && image_tokens(images) < text_tokens(char_count, chars_per_token)
}

/// OpenAI's high-detail pre-tile resize: first scale to fit inside a 2048×2048
/// box (downscale only), then scale so the SHORTEST side is 768px. Tiling is
/// counted on the resized dimensions — skipping this under-counts tiles for thin
/// pages (a 728px-tall page gets upscaled to a 768 short side), which would let
/// an unprofitable image slip through the gate.
fn openai_resized(w: u32, h: u32) -> (f64, f64) {
    let (mut w, mut h) = (w as f64, h as f64);
    if w <= 0.0 || h <= 0.0 {
        return (w, h);
    }
    let fit = (2048.0 / w).min(2048.0 / h).min(1.0);
    w *= fit;
    h *= fit;
    let short = w.min(h);
    if short > 0.0 {
        let s = 768.0 / short;
        w *= s;
        h *= s;
    }
    (w, h)
}

/// OpenAI/GPT vision-token estimate. OpenAI bills by TILES, not pixels: after the
/// high-detail resize the image is covered in 512×512 tiles at `85 + 170·tiles`.
/// This is the gpt-4o-style tile model (over-states cost for the newer patch-
/// billed gpt-5 family, which only biases toward pass-through — safe). We do not
/// port the full per-model profile table; a new-model retune can add it later.
pub fn gpt_image_tokens(images: &[RenderedImage]) -> u64 {
    images
        .iter()
        .map(|im| {
            let (w, h) = openai_resized(im.width, im.height);
            let tiles = (w / 512.0).ceil() as u64 * (h / 512.0).ceil() as u64;
            85 + 170 * tiles
        })
        .sum()
}

/// GPT-path profitability: tile-billed image tokens vs text tokens.
pub fn is_gpt_profitable(
    images: &[RenderedImage],
    char_count: usize,
    chars_per_token: f64,
) -> bool {
    !images.is_empty() && gpt_image_tokens(images) < text_tokens(char_count, chars_per_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{render_text, RenderOpts};

    #[test]
    fn tiny_text_is_not_profitable() {
        let imgs = render_text("hi", RenderOpts::default());
        // A full-width page for 2 chars costs way more image tokens than text.
        assert!(!is_profitable(&imgs, 2, 4.0));
    }

    #[test]
    fn dense_slab_is_profitable() {
        // ~12k chars of dense text: image path should win.
        let big = "abcdefghij ".repeat(1100);
        let imgs = render_text(&big, RenderOpts::default());
        assert!(is_profitable(&imgs, big.len(), 4.0));
    }
}
