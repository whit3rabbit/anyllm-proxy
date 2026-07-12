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
    /// Relax SSRF protection to permit loopback + private IPs. Set true only for
    /// admin-configured managed backends whose provider is a local LLM server
    /// (Ollama/LM Studio/vLLM/...). Cloud-metadata IPs stay blocked regardless.
    pub allow_local_ssrf: bool,
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
    /// Enable text-to-image context compression (pxpipe; BACKEND=anthropic passthrough only).
    pub pxpipe_compress: bool,
    /// Anthropic passthrough only: forward the CLIENT's own incoming
    /// `Authorization`/`x-api-key` header upstream verbatim instead of the
    /// operator's configured credential, for a single-key/BYOK deployment
    /// (e.g. a Claude Pro/Max subscription's own OAuth token). Global (like
    /// `anthropic_thinking_repair` above), not per-backend: every backend of
    /// `BackendKind::Anthropic` shares one `RuntimeConfig`
    /// (`AppState::forward_client_auth_enabled()`), live-toggleable from the
    /// admin UI. See `server/passthrough.rs::client_auth_forwardable` for the
    /// per-request safeguard that also gates this on how the request itself
    /// authenticated, and `server/middleware/auth.rs::forward_client_auth_misconfigured`
    /// for the multi-static-key safeguard enforced both at startup and on
    /// every admin-API attempt to enable it live.
    pub forward_client_auth: bool,
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
