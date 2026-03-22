pub mod gemini_client;
pub mod openai_client;

use crate::config::{BackendKind, Config, TlsConfig};
use anthropic_openai_translate::{gemini, openai};
use gemini_client::{GeminiClient, GeminiClientError};
use openai_client::{OpenAIClient, OpenAIClientError};
use reqwest::Client;
use std::time::Duration;

pub(crate) const MAX_RETRIES: u32 = 3;
pub(crate) const BASE_DELAY_MS: u64 = 500;

/// Build a reqwest HTTP client with optional mTLS identity and custom CA cert.
pub(crate) fn build_http_client(tls: &TlsConfig) -> Client {
    let mut builder = Client::builder();

    if let Some((ref p12_bytes, ref password)) = tls.p12_identity {
        let identity = reqwest::Identity::from_pkcs12_der(p12_bytes, password)
            .expect("P12 identity was validated at startup");
        builder = builder.identity(identity);
    }

    if let Some(ref ca_pem) = tls.ca_cert_pem {
        let cert =
            reqwest::Certificate::from_pem(ca_pem).expect("CA cert was validated at startup");
        builder = builder.add_root_certificate(cert);
    }

    builder.build().expect("failed to build HTTP client")
}

/// Check if a status code is retryable (429 or 5xx).
pub(crate) fn is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Parse retry-after header value in seconds.
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Compute backoff delay with jitter.
pub(crate) fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra;
    }
    let base = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt));
    // Add up to 25% jitter (deterministic upper bound; true randomness not needed here)
    let jitter_ms = (base.as_millis() as u64) / 4;
    base + Duration::from_millis(jitter_ms)
}

/// Backend-agnostic client for dispatching requests to OpenAI, Vertex, or Gemini.
#[derive(Clone)]
pub enum BackendClient {
    OpenAI(OpenAIClient),
    Vertex(OpenAIClient),
    Gemini(GeminiClient),
}

/// Unified error type for all backend clients.
#[derive(Debug)]
pub enum BackendError {
    OpenAI(OpenAIClientError),
    Gemini(GeminiClientError),
}

impl BackendError {
    /// HTTP status code for API errors, None for transport/deserialization errors.
    pub fn api_error_status(&self) -> Option<u16> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, .. }) => Some(*status),
            Self::Gemini(GeminiClientError::ApiError { status, .. }) => Some(*status),
            _ => None,
        }
    }

    /// Human-readable error message.
    pub fn api_error_message(&self) -> String {
        match self {
            Self::OpenAI(e) => e.to_string(),
            Self::Gemini(e) => e.to_string(),
        }
    }

    /// Extract the upstream error message and HTTP status for API errors.
    /// Returns None for transport/deserialization errors.
    pub fn api_error_details(&self) -> Option<(&str, u16)> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, error }) => {
                Some((&error.error.message, *status))
            }
            Self::Gemini(GeminiClientError::ApiError { status, error }) => {
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
            Self::Gemini(e) => write!(f, "{e}"),
        }
    }
}

impl From<OpenAIClientError> for BackendError {
    fn from(e: OpenAIClientError) -> Self {
        Self::OpenAI(e)
    }
}

impl From<GeminiClientError> for BackendError {
    fn from(e: GeminiClientError) -> Self {
        Self::Gemini(e)
    }
}

impl BackendClient {
    pub fn new(config: &Config) -> Self {
        match config.backend {
            BackendKind::OpenAI => Self::OpenAI(OpenAIClient::new(config)),
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(config)),
            BackendKind::Gemini => Self::Gemini(GeminiClient::new(config)),
        }
    }

    /// Which backend variant this client targets.
    pub fn backend_kind(&self) -> BackendKind {
        match self {
            Self::OpenAI(_) => BackendKind::OpenAI,
            Self::Vertex(_) => BackendKind::Vertex,
            Self::Gemini(_) => BackendKind::Gemini,
        }
    }

    /// Send a non-streaming chat completion request (OpenAI/Vertex only).
    pub async fn chat_completion(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(openai::ChatCompletionResponse, u16), BackendError> {
        match self {
            Self::OpenAI(c) | Self::Vertex(c) => c.chat_completion(req).await.map_err(Into::into),
            Self::Gemini(_) => unreachable!("chat_completion called on Gemini backend"),
        }
    }

    /// Send a streaming chat completion request (OpenAI/Vertex only).
    pub async fn chat_completion_stream(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, BackendError> {
        match self {
            Self::OpenAI(c) | Self::Vertex(c) => {
                c.chat_completion_stream(req).await.map_err(Into::into)
            }
            Self::Gemini(_) => unreachable!("chat_completion_stream called on Gemini backend"),
        }
    }

    /// Send a non-streaming generateContent request (Gemini only).
    pub async fn generate_content(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<(gemini::GenerateContentResponse, u16), BackendError> {
        match self {
            Self::Gemini(c) => c.generate_content(req, model).await.map_err(Into::into),
            _ => unreachable!("generate_content called on non-Gemini backend"),
        }
    }

    /// Send a streaming generateContent request (Gemini only).
    pub async fn generate_content_stream(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<reqwest::Response, BackendError> {
        match self {
            Self::Gemini(c) => c
                .generate_content_stream(req, model)
                .await
                .map_err(Into::into),
            _ => unreachable!("generate_content_stream called on non-Gemini backend"),
        }
    }
}
