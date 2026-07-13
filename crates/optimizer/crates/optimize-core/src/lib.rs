//! `anyllm_optimize_core` — Frozen-Frontier Extractive Compression (FFEC), core.
//!
//! Pure algorithm over a provider-agnostic IR. No IO, no serde, no ML, no clocks,
//! no global mutable state. Provider JSON ingest/emit and heuristics live in
//! `anyllm_optimize_passes`; the ONNX scorer lives in `anyllm_optimize_scorer`.
//!
//! Design (see `docs/ALGO.md`): compression is a PURE function of a single
//! message's bytes, so recompressing the same message yields identical bytes across
//! turns — cache-stable by construction. A monotone, batched frontier bounds cache
//! invalidation to one write per K turns. Fail-open is absolute: any error forwards
//! the original request untouched.
//!
//! Entry point: [`optimize`]. Runs end-to-end with [`UniformScorer`] (for heuristic mode)
//! or [`TokenScorer`] implementations like `LlmLingua2Scorer` (via `anyllm_optimize_scorer`),
//! utilizing a structural [`segment`]er and a pricing-based cost gate to ensure efficiency.

mod budget;
mod budget_planner;
mod compress;
mod cost;
mod dedup;
mod edit;
mod error;
mod frontier;
mod normalize;
mod orchestrator;
mod policy;
mod render;
mod report;
mod segment;
mod select;
mod traits;
mod types;
mod workspace;

pub use budget::HeuristicBudgetCounter;
pub use budget_planner::BudgetPlanner;
pub use compress::compress_message;
pub use cost::{net_cost_delta_usd, should_apply};
pub use dedup::dedup_pass;
pub use edit::{Edit, EditError, EditScript};
pub use error::{OptimizeError, ScoreError};
pub use frontier::{frontier, FrontierPolicy};
pub use normalize::{normalize_buffer, normalize_pass};
pub use orchestrator::{optimize, optimize_for_route, OptimizeOutcome};
pub use policy::{CompressionPolicy, OptimizationPolicy, Policy, RatioTable, RouteOverride};
pub use render::{render, RenderedConversation, RenderedMessage};
pub use report::{Mode, OptimizationReport};
pub use segment::{segment, SegKind, Segment};
pub use select::{emit_edits, quantize, select_keep, split_words, ForceRules, Word};
pub use traits::{
    BudgetCounter, CacheModel, CacheStrategy, Pass, Pricing, TokenScorer, UniformScorer,
};
pub use types::{BufferId, ContentBlock, Conversation, Message, PolicyVersion, Protection, Role};
pub use workspace::Workspace;
