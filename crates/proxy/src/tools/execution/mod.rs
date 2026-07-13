//! Tool execution: the shared types plus the guardrail, runner, and helper
//! submodules that implement partitioning, parallel execution, and the bounded
//! non-streaming tool loop.

use std::time::Duration;

use serde_json::Value;

use crate::tools::trace::ToolOutcome;

mod guardrail;
mod helpers;
mod runner;

pub use guardrail::{
    denied_tool_results, guardrail_nudge_results, partition_and_nudge, partition_tool_calls,
};
pub use helpers::{
    extract_tool_calls, response_to_assistant_message, tool_results_to_user_message,
};
pub use runner::{execute_tool_calls, execute_tool_calls_timed, is_duplicate, maybe_execute_tools};

/// Engine state needed by `maybe_execute_tools`. Re-exported alias so callers
/// do not need to reach into `server::state`.
pub use crate::server::state::ToolEngineState;

/// A tool call extracted from an LLM response.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Result of a single tool execution, tied back to the original call.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub outcome: ToolOutcome,
}

/// Configuration for the execution loop.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_iterations: usize,
    pub tool_timeout: Duration,
    pub total_timeout: Duration,
    pub max_tool_calls_per_turn: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1,
            tool_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(300),
            max_tool_calls_per_turn: 16,
        }
    }
}

#[cfg(test)]
mod tests;
