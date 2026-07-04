// Tool execution engine and registry

pub mod builtin;
pub mod execution;
pub mod guardrails;
pub mod mcp;
pub mod policy;
pub mod registry;
pub mod trace;

pub use execution::{maybe_execute_tools, LoopConfig, ToolCall, ToolResult};
pub use guardrails::{
    evaluate_tool_guardrails, resolve_runtime_guardrails, resolve_runtime_guardrails_locked,
    ToolGuardrailConfig, ToolGuardrailMode, ToolGuardrailNudge, ToolGuardrailRequestState,
};
pub use mcp::McpServerManager;
pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
pub use trace::{LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome};
