//! Text -> grayscale glyph pages. Port of the core of pxpipe's `render.ts`
//! (single-column path). Wrap at `cols`, blit 8x8 glyphs into a framebuffer,
//! invert to black-on-white, split into pages capped at `max_height_px`.
//!
//! Deterministic: a pure function of (text, opts). No color, no reflow yet —
//! P1 proves the pixel path is byte-stable end to end. Multi-col / reflow /
//! per-role color come in later phases.

use super::atlas::{glyph, GLYPH_H, GLYPH_W};
use super::png;

/// Horizontal padding on each side, px.
pub const PAD_X: usize = 4;
/// Vertical padding on each side, px.
pub const PAD_Y: usize = 4;
/// Default wrap width in columns.
pub const DEFAULT_COLS: usize = 100;
/// Default page-height ceiling in px (Anthropic-safe, mirrors pxpipe's 728).
pub const DEFAULT_MAX_HEIGHT_PX: usize = 728;

/// Cell advance (currently == glyph size; a later phase can add spacing).
const CELL_W: usize = GLYPH_W;
const CELL_H: usize = GLYPH_H;

#[derive(Clone, Copy, Debug)]
pub struct RenderOpts {
    pub cols: usize,
    pub max_height_px: usize,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            cols: DEFAULT_COLS,
            max_height_px: DEFAULT_MAX_HEIGHT_PX,
        }
    }
}

/// One rendered page.
#[derive(Clone, Debug)]
pub struct RenderedImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Input codepoints laid down on this page (spaces count; wrap breaks don't).
    pub chars_rendered: usize,
    /// Codepoints with no atlas glyph, drawn as blank cells.
    pub dropped: usize,
}

/// Wrap `text` into visual lines at `cols`. Hard `\n` ends a line; long lines
/// soft-wrap by codepoint (monospace, every codepoint = 1 cell in P1).
fn wrap_lines(text: &str, cols: usize) -> Vec<Vec<char>> {
    let mut lines: Vec<Vec<char>> = Vec::new();
    for raw in text.split('\n') {
        let mut cur: Vec<char> = Vec::new();
        for ch in raw.chars() {
            if cur.len() == cols {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push(ch);
        }
        lines.push(cur); // preserves blank lines
    }
    lines
}

/// Rows of glyphs that fit under the height ceiling.
fn rows_per_page(max_height_px: usize) -> usize {
    ((max_height_px.saturating_sub(2 * PAD_Y)) / CELL_H).max(1)
}

/// Blit one page's lines into a fresh black framebuffer, invert to
/// black-on-white, PNG-encode. Returns the image plus (chars, dropped).
fn render_page(lines: &[Vec<char>], cols: usize) -> RenderedImage {
    let width = 2 * PAD_X + cols * CELL_W;
    let height = 2 * PAD_Y + lines.len().max(1) * CELL_H;
    let mut fb = vec![0u8; width * height];
    let mut chars_rendered = 0usize;
    let mut dropped = 0usize;

    for (row, line) in lines.iter().enumerate() {
        let y0 = PAD_Y + row * CELL_H;
        for (col, &ch) in line.iter().enumerate() {
            chars_rendered += 1;
            let x0 = PAD_X + col * CELL_W;
            let Some(bitmap) = glyph(ch) else {
                if ch != ' ' {
                    dropped += 1;
                }
                continue; // blank cell
            };
            for (gy, rowbits) in bitmap.iter().enumerate() {
                for gx in 0..GLYPH_W {
                    if rowbits & (1 << gx) != 0 {
                        fb[(y0 + gy) * width + (x0 + gx)] = 255;
                    }
                }
            }
        }
    }

    // Invert: ink (255) -> 0 black, background (0) -> 255 white.
    for p in fb.iter_mut() {
        *p = 255 - *p;
    }

    RenderedImage {
        png: png::encode_gray(&fb, width as u32, height as u32),
        width: width as u32,
        height: height as u32,
        chars_rendered,
        dropped,
    }
}

/// Render `text` to one or more grayscale PNG pages.
pub fn render_text(text: &str, opts: RenderOpts) -> Vec<RenderedImage> {
    // No content -> no image. (wrap_lines always yields >=1 line even for "", so
    // guard on the input here rather than on the wrapped result.)
    if text.is_empty() {
        return Vec::new();
    }
    let cols = opts.cols.max(1);
    let lines = wrap_lines(text, cols);
    let per_page = rows_per_page(opts.max_height_px);
    lines
        .chunks(per_page)
        .map(|page| render_page(page, cols))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_respects_cols_and_newlines() {
        let lines = wrap_lines("abcdef\nxy", 4);
        // "abcdef" -> ["abcd","ef"], then "xy"
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], vec!['a', 'b', 'c', 'd']);
        assert_eq!(lines[1], vec!['e', 'f']);
        assert_eq!(lines[2], vec!['x', 'y']);
    }

    #[test]
    fn renders_and_is_deterministic() {
        let a = render_text("hello world\nfn main() {}", RenderOpts::default());
        let b = render_text("hello world\nfn main() {}", RenderOpts::default());
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].png, b[0].png, "render must be byte-stable");
        assert!(a[0].dropped == 0);
    }

    #[test]
    fn pages_split_on_height() {
        let opts = RenderOpts {
            cols: 10,
            max_height_px: 2 * PAD_Y + 3 * CELL_H, // 3 rows per page
        };
        let text = (0..7)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = render_text(&text, opts);
        assert_eq!(pages.len(), 3); // 7 lines / 3 per page = 3 pages
    }
}
