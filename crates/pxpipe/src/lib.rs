//! # anyllm_pxpipe
//!
//! Text-to-image context compression for the anyllm proxy — a Rust port of the
//! core of [pxpipe](https://github.com/teamchong/pxpipe).
//!
//! The idea: render the stable, token-heavy parts of a request (system prompt,
//! tool defs, history) into dense PNG glyph images and swap them into the body
//! as image blocks. Vision models read the text off the image; the provider
//! bills the image far cheaper than the equivalent text, saving input tokens.
//!
//! This crate is **IO-free** (like `anyllm_translate`): pure transforms over
//! `serde_json::Value` plus the renderer. The proxy owns the opt-in gating,
//! the vision-capability check, and the HTTP plumbing.
//!
//! Phase status: **P1** ships the deterministic renderer only. The
//! request-transform surface (`transform_anthropic`, GPT paths) lands in P2+.

pub mod render;
pub mod transform;

pub use render::{render_text, RenderOpts, RenderedImage};
pub use transform::{
    transform_anthropic, transform_openai_chat, AnthropicOpts, GptOpts, TransformInfo,
};
