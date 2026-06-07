/// Wire format / HTTP client strategy used to communicate with a provider.
/// Maps to proxy's `BackendKind` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProviderProtocol {
    /// Standard OpenAI Chat Completions (Groq, Together, Mistral, etc.)
    OpenAICompat,
    /// Azure OpenAI: deployment-based URLs, `api-key` header auth
    AzureOpenAI,
    /// Vertex AI: project/region URL construction, GCP auth
    VertexAI,
    /// Google AI Studio (Gemini) via `/openai` suffix
    GeminiOpenAI,
    /// Gemini native `generateContent` API (no OpenAI translation)
    GeminiNative,
    /// Anthropic Messages API passthrough (no translation)
    AnthropicNative,
    /// AWS Bedrock: SigV4 signing + Event Stream binary framing
    BedrockNative,
    /// Not yet implemented; reject at config time with a helpful message
    Custom,
}

/// How the provider expects authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuthKind {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-goog-api-key: <key>` (Vertex, Gemini)
    GoogleApiKey,
    /// `api-key: <key>` header (Azure OpenAI)
    AzureApiKey,
    /// AWS SigV4 (access key + secret + optional session token)
    AwsSigV4,
    /// No key required (local/self-hosted)
    None,
}

/// Implementation maturity of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProviderStatus {
    /// HTTP client exists and has been live-tested.
    Implemented,
    /// HTTP client wired up but not yet live-tested.
    Wired,
    /// Metadata only. Routed through an existing compatible client at runtime
    /// (e.g., OpenAI-compat providers use `OpenAIClient`).
    Stub,
}

/// Capabilities advertised for a provider (endpoint-level, not model-level).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilities {
    pub chat_completions: bool,
    pub streaming: bool,
    pub tool_use: bool,
    pub embeddings: bool,
    pub vision: bool,
    pub batch: bool,
}

/// Metadata for a single provider. No HTTP clients, no I/O.
///
/// All fields are `'static` so provider definitions can be compile-time constants.
#[derive(Debug, Clone)]
pub struct ProviderDef {
    /// Short stable identifier matching LiteLLM convention (e.g. `"groq"`, `"together_ai"`).
    pub id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Default API base URL. May be empty for providers requiring per-deployment config.
    pub default_base_url: &'static str,
    /// Wire format / HTTP client strategy.
    pub protocol: ProviderProtocol,
    /// Authentication method.
    pub auth: AuthKind,
    /// Implementation status.
    pub status: ProviderStatus,
    /// Environment variable(s) that supply the API key, in priority order.
    /// First entry is the canonical name; others are aliases.
    pub env_vars: &'static [&'static str],
    /// LiteLLM YAML prefix (e.g. `"groq/"`) for `parse_provider_model()`.
    /// Must include the trailing slash if present in LiteLLM convention.
    pub litellm_prefix: &'static str,
    /// Endpoint-level capabilities common to all models on this provider.
    pub capabilities: ProviderCapabilities,
}
