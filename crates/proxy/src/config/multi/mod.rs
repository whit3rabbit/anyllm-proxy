use super::tls::TlsConfig;
use super::types::{BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat};
use indexmap::IndexMap;

/// Per-backend configuration. Each entry in `[backends.*]` deserializes into this.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Which provider type this backend uses (OpenAI, Vertex, Gemini, Anthropic).
    pub kind: BackendKind,
    /// Canonical provider id used for provider-specific normalization policy.
    pub provider_id: Option<String>,
    /// API key for authentication. Resolved from env vars via `env:VAR_NAME` syntax.
    pub api_key: String,
    /// Base URL of the backend API (e.g., `https://api.openai.com`).
    pub base_url: String,
    /// Which OpenAI API format to use (Chat Completions or Responses).
    pub api_format: OpenAIApiFormat,
    /// Anthropic-to-backend model name mapping.
    pub model_mapping: ModelMapping,
    /// Optional mTLS and custom CA configuration.
    pub tls: TlsConfig,
    /// How to authenticate to this backend (Bearer token or Google API key).
    pub backend_auth: BackendAuth,
    /// Whether to log request/response bodies at debug level.
    pub log_bodies: bool,
    /// Strip `stream_options` from streaming requests. Needed for local LLMs
    /// (older Ollama, text-generation-webui, LM Studio) that reject unknown
    /// fields with HTTP 400.
    pub omit_stream_options: bool,
    /// Wall-clock cap for streaming responses in seconds. 0 = disabled.
    pub stream_timeout_secs: u64,
    /// AWS credentials for Bedrock backend. None for all other backends.
    pub bedrock_credentials: Option<aws_credential_types::Credentials>,
}

/// Top-level multi-backend configuration loaded from TOML.
/// Enables routing requests to different backends by route prefix.
#[derive(Debug, Clone)]
pub struct MultiConfig {
    /// Port the proxy listens on (default: 3000).
    pub listen_port: u16,
    /// Whether to log request/response bodies at debug level (global default).
    pub log_bodies: bool,
    /// Redact detected secrets from upstream JSON/text request payloads.
    pub redact_secrets: bool,
    /// Enable Anthropic thinking-block record-and-restore repair (BACKEND=anthropic passthrough only).
    pub anthropic_thinking_repair: bool,
    /// Backend name used when no route prefix matches.
    pub default_backend: String,
    /// Ordered map: key = route prefix (e.g. "openai"), value = backend config.
    pub backends: IndexMap<String, BackendConfig>,
    /// See Config::expose_degradation_warnings.
    pub expose_degradation_warnings: bool,
}

mod loader;
mod toml_loader;

#[cfg(test)]
mod tests;

pub use loader::LoadResult;
