//! `anyllm_rtk` — command-aware tool-output compression (RTK port).
//!
//! IO-free, deterministic transforms over a `serde_json::Value` request body:
//! rewrite the inner text of tool-output blocks (`tool_result` / `role:tool`)
//! using a catalog of declarative filters, leaving all other content untouched.
//! Determinism keeps upstream prompt caches warm; `cache_control`-marked blocks
//! are preserved byte-for-byte.
//!
//! Ported from OmniRoute's RTK engine (MIT). See `filters/ATTRIBUTION.md`.

mod dedup;
mod detector;
mod engine;
mod filter;
mod line_filter;
mod smart_truncate;
mod transform;

pub use engine::{filters, match_filter, process_rtk_text, ProcessResult};
pub use filter::{InlineTest, RtkFilter};
pub use line_filter::apply_line_filter;
pub use transform::{transform_anthropic, transform_openai_chat, RtkInfo};
