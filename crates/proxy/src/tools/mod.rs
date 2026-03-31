// Tool execution engine and registry

pub mod builtin;
pub mod execution;
pub mod policy;
pub mod registry;
pub mod trace;

pub use execution::{LoopConfig, ToolCall, ToolResult};
pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
pub use trace::{LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome};
