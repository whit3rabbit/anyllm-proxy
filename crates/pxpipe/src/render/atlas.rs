//! Glyph atlas. Wraps the embedded public-domain `font8x8` byte tables so the
//! rest of the renderer only sees "codepoint -> 8x8 bitmap or None".
//!
//! pxpipe ships its own base64 atlas (Spleen 5x8 + Unifont) for maximum density.
//! We start with font8x8's 8x8 basic + block + box + Latin coverage: bigger
//! glyphs (more image tokens per char) but zero hand-transcription and fully
//! deterministic. Cell dimensions are exported so the density can be retuned
//! later (P2) without touching call sites.

use font8x8::UnicodeFonts;

/// Glyph width in pixels.
pub const GLYPH_W: usize = 8;
/// Glyph height in pixels.
pub const GLYPH_H: usize = 8;

/// Look up the 8x8 bitmap for a codepoint. Each returned byte is one row, with
/// bit 0 (LSB) = leftmost pixel (font8x8 convention). `None` for codepoints not
/// in the atlas — the caller renders a blank cell and counts the drop.
pub fn glyph(ch: char) -> Option<[u8; 8]> {
    // font8x8 partitions coverage across several tables; try the common ones in
    // order of likelihood for code/prose content.
    font8x8::BASIC_FONTS
        .get(ch)
        .or_else(|| font8x8::LATIN_FONTS.get(ch))
        .or_else(|| font8x8::BOX_FONTS.get(ch))
        .or_else(|| font8x8::BLOCK_FONTS.get(ch))
        .or_else(|| font8x8::GREEK_FONTS.get(ch))
}

/// True if `ch` has a glyph (used by telemetry to distinguish "rendered blank
/// space" from "dropped unknown codepoint").
#[allow(dead_code)] // P2+ telemetry surface
pub fn has_glyph(ch: char) -> bool {
    // Space is intentionally blank but not "dropped".
    ch == ' ' || glyph(ch).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_letters_present() {
        for ch in ['A', 'z', '0', '/', '{', '_', '-'] {
            assert!(glyph(ch).is_some(), "missing glyph for {ch:?}");
        }
    }

    #[test]
    fn glyph_is_8_rows() {
        let g = glyph('A').unwrap();
        assert_eq!(g.len(), 8);
    }
}
