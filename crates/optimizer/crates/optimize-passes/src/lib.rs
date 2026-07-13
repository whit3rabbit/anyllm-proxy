//! `anyllm_optimize_passes` — everything that touches provider JSON or pricing.
//!
//! - `adapter::{openai, anthropic}`: build the provider-agnostic IR from a request
//!   `serde_json::Value` and write a compressed [`RenderedConversation`] back in place
//!   (preserving all fields the optimizer does not touch).
//! - `cache_strategy`: per-provider [`CacheStrategy`] (frontier/breakpoint/pricing).
//! - `cost_gate`: pricing tables + re-export of the pure cost gate.
//! - `tool_result`: JSON-value compression of verbose tool outputs (fully implemented).
//!
//! The pure algorithm (frontier, selection, `optimize()`) lives in
//! `anyllm_optimize_core`.

pub mod adapter;
pub mod cache_strategy;
pub mod cost_gate;
pub mod tool_result;

pub use anyllm_optimize_core::HeuristicBudgetCounter;
pub use cache_strategy::{AnthropicStrategy, DisabledStrategy, GeminiStrategy, OpenAiStrategy};
pub use cost_gate::{anthropic_pricing, gemini_pricing, openai_pricing};
pub use tool_result::compress_tool_result;

// Re-export the core symbols a caller (proxy shim / cli) needs so they can depend on
// just `anyllm_optimize_passes` for the common path.
pub use anyllm_optimize_core::{
    optimize, Conversation, Mode, OptimizationReport, OptimizeOutcome, Policy,
    RenderedConversation, TokenScorer, UniformScorer, Workspace,
};
