// Shared state types for request handlers: AppState, AnthropicJson, ResolvedModel, etc.
// Extracted from routes.rs so consumers can import state independently of the router setup.

pub(crate) mod anthropic_json;
pub(crate) mod app_state;
pub(crate) mod compression;
pub(crate) mod concurrency;
pub(crate) mod resolved_model;
pub(crate) mod tool_engine;

pub(crate) use anthropic_json::AnthropicJson;
pub use app_state::AppState;
pub(crate) use concurrency::{ConcurrencyPermit, GlobalState};
pub(crate) use resolved_model::ResolvedModel;
pub use tool_engine::ToolEngineState;
