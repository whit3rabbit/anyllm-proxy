use std::fmt;

/// Path suffix appended to Gemini base URL to reach its OpenAI-compatible endpoint.
pub(crate) const GEMINI_OPENAI_PATH: &str = "/openai";

/// Which upstream backend the proxy targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    OpenAI,
    AzureOpenAI,
    Vertex,
    Gemini,
    Anthropic,
    Bedrock,
}

/// Which OpenAI API format to use (only relevant when BACKEND=openai).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIApiFormat {
    /// Chat Completions API (default)
    Chat,
    /// Responses API
    Responses,
}

/// How the proxy authenticates to the upstream backend.
#[derive(Clone)]
pub enum BackendAuth {
    /// `Authorization: Bearer {token}` (OpenAI, Vertex OAuth)
    BearerToken(String),
    /// `x-goog-api-key: {key}` (Vertex API key)
    GoogleApiKey(String),
    /// `api-key: {key}` (Azure OpenAI)
    AzureApiKey(String),
}

impl fmt::Debug for BackendAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BearerToken(_) => write!(f, "BearerToken([REDACTED])"),
            Self::GoogleApiKey(_) => write!(f, "GoogleApiKey([REDACTED])"),
            Self::AzureApiKey(_) => write!(f, "AzureApiKey([REDACTED])"),
        }
    }
}

/// Maps Anthropic model names to OpenAI model names.
/// Pattern: "haiku" -> small_model, "sonnet"/"opus" -> big_model.
/// Unrecognized models pass through with a warning.
#[derive(Debug, Clone)]
pub struct ModelMapping {
    pub big_model: String,
    pub small_model: String,
}

impl ModelMapping {
    /// Load model mapping from `BIG_MODEL` / `SMALL_MODEL` env vars with OpenAI defaults.
    pub fn from_env() -> Self {
        Self::from_env_with_defaults("gpt-4o", "gpt-4o-mini")
    }

    /// Load model mapping from env vars, falling back to the provided defaults.
    /// Each backend calls this with its own defaults (e.g., Gemini uses `gemini-2.5-pro`).
    pub fn from_env_with_defaults(big_default: &str, small_default: &str) -> Self {
        Self {
            big_model: std::env::var("BIG_MODEL").unwrap_or_else(|_| big_default.into()),
            small_model: std::env::var("SMALL_MODEL").unwrap_or_else(|_| small_default.into()),
        }
    }

    /// Map an Anthropic model name to the configured OpenAI model.
    pub fn map_model(&self, model: &str) -> String {
        // ASCII case-insensitive substring check avoids allocating a lowercase copy.
        let bytes = model.as_bytes();
        if super::helpers::contains_ignore_ascii_case(bytes, b"haiku") {
            self.small_model.clone()
        } else if super::helpers::contains_ignore_ascii_case(bytes, b"sonnet")
            || super::helpers::contains_ignore_ascii_case(bytes, b"opus")
        {
            self.big_model.clone()
        } else {
            tracing::warn!(model = %model, "unrecognized model name, passing through unchanged");
            model.to_string()
        }
    }
}
