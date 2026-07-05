//! Text-to-PNG rendering. Deterministic by construction (fixed font, fixed
//! encoder) so rendered images stay byte-stable across turns and keep the
//! Anthropic prompt cache warm.

mod atlas;
mod png;
mod text;

pub use text::{render_text, RenderOpts, RenderedImage};
