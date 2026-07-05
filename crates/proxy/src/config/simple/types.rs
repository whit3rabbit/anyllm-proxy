use serde::Deserialize;
use std::collections::HashMap;

/// Top-level simple config document.
#[derive(Debug, Deserialize)]
pub struct SimpleConfig {
    /// Routing strategy for all models. Case-insensitive.
    /// Accepted values: round-robin, least-busy, latency-based, weighted, cost-based.
    /// Default: round-robin.
    #[serde(default)]
    pub routing_strategy: Option<String>,
    /// Proxy listen port. Default: 3000.
    #[serde(default)]
    pub listen_port: Option<u16>,
    /// Log request/response bodies at debug level. Default: false.
    #[serde(default)]
    pub log_bodies: Option<bool>,
    /// Redact detected secrets from upstream JSON/text request payloads.
    #[serde(default)]
    pub redact_secrets: Option<bool>,
    /// Enable Anthropic thinking-block record-and-restore repair (BACKEND=anthropic passthrough only).
    #[serde(default)]
    pub anthropic_thinking_repair: Option<bool>,
    /// Enable text-to-image context compression (pxpipe; BACKEND=anthropic passthrough only).
    #[serde(default)]
    pub pxpipe_compress: Option<bool>,
    /// List of model deployments.
    #[serde(default)]
    pub models: Vec<SimpleModelEntry>,
    #[serde(default)]
    pub tool_execution: Option<ToolExecutionConfig>,
    #[serde(default)]
    pub builtin_tools: Option<HashMap<String, BuiltinToolConfig>>,
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServerConfig>>,
}

/// A model entry: either a string shorthand or a full struct.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SimpleModelEntry {
    /// String shorthand: "model-name" or "provider/model-name".
    Shorthand(String),
    /// Full form with all fields. Boxed to reduce enum size.
    Full(Box<SimpleModelFull>),
}

/// Full model entry with all optional fields.
#[derive(Debug, Deserialize)]
pub struct SimpleModelFull {
    /// Virtual model name clients send in requests. Defaults to `model` if omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// Actual model name forwarded to the backend.
    pub model: String,
    /// Backend provider. Default: "openai".
    #[serde(default)]
    pub provider: Option<String>,
    /// Static weight for weighted routing. Default: 1.
    #[serde(default)]
    pub weight: Option<u32>,
    /// Per-deployment requests-per-minute limit.
    #[serde(default)]
    pub rpm: Option<u32>,
    /// Per-deployment tokens-per-minute limit.
    #[serde(default)]
    pub tpm: Option<u64>,
    /// API key override. When absent, falls back to the standard env var for the provider.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL override. When absent, uses the provider default.
    #[serde(default)]
    pub api_base: Option<String>,
    // Azure-specific
    #[serde(default)]
    pub deployment: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    // Vertex-specific
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    // Bedrock-specific
    #[serde(default)]
    pub aws_region: Option<String>,
    #[serde(default)]
    pub aws_access_key_id: Option<String>,
    #[serde(default)]
    pub aws_secret_access_key: Option<String>,
}

/// Tool execution loop configuration.
#[derive(Debug, Deserialize)]
pub struct ToolExecutionConfig {
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default)]
    pub total_timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_tool_calls_per_turn: Option<usize>,
    /// Optional Forge-style advisory guardrails for model-produced tool calls.
    /// Accepted values: disabled, standard, off, on, false, true, 0, 1.
    #[serde(default)]
    pub guardrails: Option<String>,
    /// Maximum string payload size before write/edit guardrails nudge.
    #[serde(default)]
    pub max_write_payload_bytes: Option<usize>,
}

/// Configuration for a single builtin tool.
#[derive(Debug, Deserialize)]
pub struct BuiltinToolConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// For read_file: restrict reads to files under these absolute directory paths.
    /// If empty or absent, read_file is disabled. Use ["/"] to explicitly allow unrestricted reads.
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
}

pub(crate) fn default_true() -> bool {
    true
}

/// MCP server configuration entry.
#[derive(Debug, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub policy: Option<String>,
}

/// Tool-related config extracted from SimpleConfig, passed up to main.rs
/// so it can build ToolEngineState without re-parsing the config file.
#[derive(Debug)]
pub struct ToolStartupConfig {
    pub tool_execution: Option<ToolExecutionConfig>,
    pub builtin_tools: Option<HashMap<String, BuiltinToolConfig>>,
    pub mcp_servers: Option<Vec<McpServerConfig>>,
}

impl ToolStartupConfig {
    /// Returns true when at least one tool-related section was present in the config.
    /// Used to decide whether to construct a ToolEngineState at all.
    pub fn has_any(&self) -> bool {
        self.tool_execution.is_some() || self.builtin_tools.is_some() || self.mcp_servers.is_some()
    }
}

/// Result from parsing a simple YAML config file.
pub struct SimpleParsed {
    pub multi_config: crate::config::MultiConfig,
    pub router: crate::config::model_router::ModelRouter,
    /// Tool-related sections extracted from the config. None-valued when no tool
    /// sections were present (callers should check `has_any()` before using).
    pub tool_config: ToolStartupConfig,
}
