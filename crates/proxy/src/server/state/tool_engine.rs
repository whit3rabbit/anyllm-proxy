use std::sync::Arc;

/// Shared state for tool execution, stored in AppState.
#[derive(Clone)]
pub struct ToolEngineState {
    pub registry: Arc<crate::tools::ToolRegistry>,
    pub policy: Arc<crate::tools::ToolExecutionPolicy>,
    pub loop_config: crate::tools::LoopConfig,
    pub guardrails: crate::tools::ToolGuardrailConfig,
    pub mcp_manager: Option<Arc<crate::tools::McpServerManager>>,
}
