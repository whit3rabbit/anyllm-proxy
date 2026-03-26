/// Passthrough client forwarding Anthropic requests as-is to upstream Anthropic API.
pub mod anthropic_client;
/// reqwest client for OpenAI-compatible Chat Completions and Responses APIs with retry/backoff.
pub mod openai_client;

use crate::config::{BackendAuth, BackendConfig, BackendKind, Config, OpenAIApiFormat, TlsConfig};
use anthropic_client::{AnthropicClient, AnthropicClientError};
use openai_client::{OpenAIClient, OpenAIClientError};

// Re-export from the client crate so existing code paths (streaming, routes, etc.) keep working.
pub use anyllm_client::rate_limit::RateLimitHeaders;
pub use anyllm_client::retry::{
    backoff_delay, is_retryable, parse_retry_after, RetryableError, MAX_RETRIES,
};
pub use anyllm_client::sse::{find_double_newline, MAX_SSE_BUFFER_SIZE};

use anyllm_client::http::HttpClientConfig;

/// Build a reqwest HTTP client from proxy TlsConfig (adapter to client crate).
pub(crate) fn build_http_client(tls: &TlsConfig) -> reqwest::Client {
    let config = HttpClientConfig {
        p12_identity: tls.p12_identity.clone(),
        ca_cert_pem: tls.ca_cert_pem.clone(),
        ssrf_protection: true,
        ..Default::default()
    };
    anyllm_client::build_http_client(&config)
}

/// Send a POST request with retry on 429/5xx. Returns the raw successful response.
/// Adapter that maps BackendAuth to the client crate's RequestAuth.
pub(crate) async fn send_with_retry<E: RetryableError>(
    client: &reqwest::Client,
    url: &str,
    auth: &BackendAuth,
    body: &impl serde::Serialize,
    label: &str,
) -> Result<reqwest::Response, E> {
    let request_auth = match auth {
        BackendAuth::BearerToken(token) => anyllm_client::retry::RequestAuth::Bearer(token),
        BackendAuth::GoogleApiKey(key) => anyllm_client::retry::RequestAuth::Header {
            name: "x-goog-api-key",
            value: key,
        },
        BackendAuth::AzureApiKey(key) => anyllm_client::retry::RequestAuth::Header {
            name: "api-key",
            value: key,
        },
    };
    anyllm_client::retry::send_with_retry(client, url, &request_auth, body, label).await
}

/// Backend-agnostic client for dispatching requests to OpenAI, Vertex, Gemini, or Anthropic.
/// Callers pattern-match on the enum variants to access the typed inner clients directly.
#[derive(Clone)]
pub enum BackendClient {
    OpenAI(OpenAIClient),
    /// Same HTTP client as OpenAI, but targets the Responses API endpoint
    /// with a different request/response shape. Separate variant so callers
    /// can pattern-match on the API format.
    OpenAIResponses(OpenAIClient),
    /// Azure OpenAI: same Chat Completions format, different auth and URL scheme.
    AzureOpenAI(OpenAIClient),
    Vertex(OpenAIClient),
    /// Gemini via OpenAI-compatible endpoint (reuses OpenAI translation path).
    GeminiOpenAI(OpenAIClient),
    /// Passthrough to real Anthropic API (no translation).
    Anthropic(AnthropicClient),
}

/// Unified error type for all backend clients.
#[derive(Debug)]
pub enum BackendError {
    OpenAI(OpenAIClientError),
    Anthropic(AnthropicClientError),
}

impl BackendError {
    /// HTTP status code for API errors, None for transport/deserialization errors.
    pub fn api_error_status(&self) -> Option<u16> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, .. }) => Some(*status),
            _ => None,
        }
    }

    /// HTTP status code from an API error, or 500 for transport/deserialization errors.
    pub fn status_code(&self) -> u16 {
        self.api_error_status().unwrap_or(500)
    }

    /// Human-readable error message.
    pub fn api_error_message(&self) -> String {
        match self {
            Self::OpenAI(e) => e.to_string(),
            Self::Anthropic(e) => e.to_string(),
        }
    }

    /// Extract the upstream error message and HTTP status for API errors.
    /// Returns None for transport/deserialization errors.
    pub fn api_error_details(&self) -> Option<(&str, u16)> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, error }) => {
                Some((&error.error.message, *status))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI(e) => write!(f, "{e}"),
            Self::Anthropic(e) => write!(f, "{e}"),
        }
    }
}

impl From<OpenAIClientError> for BackendError {
    fn from(e: OpenAIClientError) -> Self {
        Self::OpenAI(e)
    }
}

impl From<AnthropicClientError> for BackendError {
    fn from(e: AnthropicClientError) -> Self {
        Self::Anthropic(e)
    }
}

impl BackendClient {
    /// Forward a raw embeddings request to the backend. No translation — model names pass through.
    /// Returns `501 Not Implemented` for the Anthropic backend (no embeddings endpoint).
    pub async fn embeddings_passthrough(
        &self,
        body: bytes::Bytes,
        content_type: &str,
    ) -> Result<(axum::http::StatusCode, axum::http::HeaderMap, bytes::Bytes), BackendError> {
        match self {
            Self::OpenAI(c)
            | Self::AzureOpenAI(c)
            | Self::Vertex(c)
            | Self::GeminiOpenAI(c)
            | Self::OpenAIResponses(c) => c
                .embeddings_passthrough(body, content_type)
                .await
                .map_err(BackendError::OpenAI),
            Self::Anthropic(_) => {
                // Anthropic has no embeddings API.
                let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                    anyllm_translate::anthropic::ErrorType::InvalidRequestError,
                    "Embeddings are not supported by the Anthropic backend.".to_string(),
                    None,
                );
                let body = serde_json::to_vec(&err).unwrap_or_default();
                Ok((
                    axum::http::StatusCode::NOT_IMPLEMENTED,
                    axum::http::HeaderMap::new(),
                    bytes::Bytes::from(body),
                ))
            }
        }
    }

    /// Create a backend client from a single-backend [`Config`].
    ///
    /// Dispatches on [`Config::backend`] and [`Config::openai_api_format`] to construct
    /// the appropriate variant (OpenAI, OpenAIResponses, Vertex, GeminiOpenAI, or Anthropic).
    pub fn new(config: &Config) -> Self {
        match config.backend {
            BackendKind::OpenAI => match config.openai_api_format {
                OpenAIApiFormat::Chat => Self::OpenAI(OpenAIClient::new(config)),
                OpenAIApiFormat::Responses => Self::OpenAIResponses(OpenAIClient::new(config)),
            },
            BackendKind::AzureOpenAI => Self::AzureOpenAI(OpenAIClient::new(config)),
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(config)),
            BackendKind::Gemini => Self::GeminiOpenAI(OpenAIClient::new(config)),
            BackendKind::Anthropic => Self::Anthropic(AnthropicClient::new(
                &config.openai_base_url,
                &config.openai_api_key,
                &config.tls,
            )),
        }
    }

    /// Construct from a per-backend config (multi-backend mode).
    pub fn from_backend_config(bc: &BackendConfig) -> Self {
        // Build a legacy Config to reuse existing OpenAI constructors.
        // This avoids duplicating URL construction logic.
        let legacy = Config {
            backend: bc.kind.clone(),
            openai_api_key: bc.api_key.clone(),
            openai_base_url: bc.base_url.clone(),
            listen_port: 0, // unused by client constructors
            model_mapping: bc.model_mapping.clone(),
            tls: bc.tls.clone(),
            backend_auth: bc.backend_auth.clone(),
            log_bodies: bc.log_bodies,
            openai_api_format: bc.api_format.clone(),
        };

        match bc.kind {
            BackendKind::OpenAI => match bc.api_format {
                OpenAIApiFormat::Chat => Self::OpenAI(OpenAIClient::new(&legacy)),
                OpenAIApiFormat::Responses => Self::OpenAIResponses(OpenAIClient::new(&legacy)),
            },
            BackendKind::AzureOpenAI => Self::AzureOpenAI(OpenAIClient::new(&legacy)),
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(&legacy)),
            BackendKind::Gemini => Self::GeminiOpenAI(OpenAIClient::new(&legacy)),
            BackendKind::Anthropic => Self::Anthropic(AnthropicClient::from_backend_config(bc)),
        }
    }
}
