pub mod anthropic_client;
pub mod openai_client;

use crate::config::{BackendAuth, BackendConfig, BackendKind, Config, OpenAIApiFormat, TlsConfig};
use anthropic_client::{AnthropicClient, AnthropicClientError};
use axum::http::{HeaderMap, HeaderName};
use openai_client::{OpenAIClient, OpenAIClientError};
use reqwest::Client;
use serde::Serialize;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) const MAX_RETRIES: u32 = 3;
pub(crate) const BASE_DELAY_MS: u64 = 500;

/// Backend error types implement this to enable the generic `send_with_retry`.
pub(crate) trait RetryableError: Sized {
    fn from_request(e: reqwest::Error) -> Self;
    fn from_api_response(status: u16, body: &str) -> Self;
}

fn apply_auth(rb: reqwest::RequestBuilder, auth: &BackendAuth) -> reqwest::RequestBuilder {
    match auth {
        BackendAuth::BearerToken(token) => rb.bearer_auth(token),
        BackendAuth::GoogleApiKey(key) => rb.header("x-goog-api-key", key),
    }
}

/// Send a POST request with retry on 429/5xx. Returns the raw successful response.
pub(crate) async fn send_with_retry<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &BackendAuth,
    body: &impl Serialize,
    label: &str,
) -> Result<reqwest::Response, E> {
    for attempt in 0..=MAX_RETRIES {
        let rb = apply_auth(client.post(url).json(body), auth);
        let response = rb.send().await.map_err(E::from_request)?;
        let status = response.status().as_u16();

        if (200..300).contains(&status) {
            return Ok(response);
        }

        if attempt < MAX_RETRIES && is_retryable(status) {
            let retry_after = parse_retry_after(response.headers());
            let delay = backoff_delay(attempt, retry_after);
            tracing::warn!(
                status,
                attempt = attempt + 1,
                max_retries = MAX_RETRIES,
                delay_ms = delay.as_millis() as u64,
                "retryable error from {label}, backing off"
            );
            // Drain body so the connection can be returned to the pool
            drop(response.bytes().await);
            sleep(delay).await;
            continue;
        }

        let text = response.text().await.unwrap_or_else(|e| {
            tracing::warn!("failed to read error response body: {e}");
            String::new()
        });
        return Err(E::from_api_response(status, &text));
    }

    unreachable!("loop runs MAX_RETRIES+1 times and always returns")
}

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

    builder
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

/// Check if a status code is retryable (429 or 5xx).
pub(crate) fn is_retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Parse retry-after header value in seconds.
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    header_str(headers, "retry-after")
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

/// Backend-agnostic client for dispatching requests to OpenAI, Vertex, Gemini, or Anthropic.
/// Callers pattern-match on the enum variants to access the typed inner clients directly.
#[derive(Clone)]
pub enum BackendClient {
    OpenAI(OpenAIClient),
    /// OpenAI Responses API format (same client, different endpoint + request/response shape).
    OpenAIResponses(OpenAIClient),
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
    pub fn new(config: &Config) -> Self {
        match config.backend {
            BackendKind::OpenAI => match config.openai_api_format {
                OpenAIApiFormat::Chat => Self::OpenAI(OpenAIClient::new(config)),
                OpenAIApiFormat::Responses => Self::OpenAIResponses(OpenAIClient::new(config)),
            },
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
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(&legacy)),
            BackendKind::Gemini => Self::GeminiOpenAI(OpenAIClient::new(&legacy)),
            BackendKind::Anthropic => Self::Anthropic(AnthropicClient::from_backend_config(bc)),
        }
    }
}

/// Rate limit headers extracted from backend responses.
#[derive(Debug, Default, Clone)]
pub struct RateLimitHeaders {
    pub requests_limit: Option<String>,
    pub requests_remaining: Option<String>,
    pub requests_reset: Option<String>,
    pub tokens_limit: Option<String>,
    pub tokens_remaining: Option<String>,
    pub tokens_reset: Option<String>,
    pub retry_after: Option<String>,
}

/// Extract a header value as a trimmed string.
fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

/// Set a header on an axum HeaderMap if the value is Some.
fn set_if_some(map: &mut HeaderMap, name: &str, value: &Option<String>) {
    if let Some(v) = value {
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            axum::http::HeaderValue::from_str(v),
        ) {
            map.insert(header_name, header_value);
        }
    }
}

impl RateLimitHeaders {
    /// Extract rate limit headers from an OpenAI (or Vertex) response.
    pub fn from_openai_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            requests_limit: header_str(headers, "x-ratelimit-limit-requests"),
            requests_remaining: header_str(headers, "x-ratelimit-remaining-requests"),
            requests_reset: header_str(headers, "x-ratelimit-reset-requests"),
            tokens_limit: header_str(headers, "x-ratelimit-limit-tokens"),
            tokens_remaining: header_str(headers, "x-ratelimit-remaining-tokens"),
            tokens_reset: header_str(headers, "x-ratelimit-reset-tokens"),
            retry_after: header_str(headers, "retry-after"),
        }
    }

    /// Inject Anthropic-format rate limit headers into a response HeaderMap.
    pub fn inject_anthropic_headers(&self, map: &mut HeaderMap) {
        set_if_some(
            map,
            "anthropic-ratelimit-requests-limit",
            &self.requests_limit,
        );
        set_if_some(
            map,
            "anthropic-ratelimit-requests-remaining",
            &self.requests_remaining,
        );
        set_if_some(
            map,
            "anthropic-ratelimit-requests-reset",
            &self.requests_reset,
        );
        set_if_some(map, "anthropic-ratelimit-tokens-limit", &self.tokens_limit);
        set_if_some(
            map,
            "anthropic-ratelimit-tokens-remaining",
            &self.tokens_remaining,
        );
        set_if_some(map, "anthropic-ratelimit-tokens-reset", &self.tokens_reset);
        set_if_some(map, "retry-after", &self.retry_after);
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    #[test]
    fn from_openai_headers_extracts_all() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", "100".parse().unwrap());
        headers.insert("x-ratelimit-remaining-requests", "99".parse().unwrap());
        headers.insert("x-ratelimit-reset-requests", "1s".parse().unwrap());
        headers.insert("x-ratelimit-limit-tokens", "40000".parse().unwrap());
        headers.insert("x-ratelimit-remaining-tokens", "39500".parse().unwrap());
        headers.insert("x-ratelimit-reset-tokens", "500ms".parse().unwrap());
        headers.insert("retry-after", "2".parse().unwrap());

        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert_eq!(rl.requests_limit.as_deref(), Some("100"));
        assert_eq!(rl.requests_remaining.as_deref(), Some("99"));
        assert_eq!(rl.requests_reset.as_deref(), Some("1s"));
        assert_eq!(rl.tokens_limit.as_deref(), Some("40000"));
        assert_eq!(rl.tokens_remaining.as_deref(), Some("39500"));
        assert_eq!(rl.tokens_reset.as_deref(), Some("500ms"));
        assert_eq!(rl.retry_after.as_deref(), Some("2"));
    }

    #[test]
    fn from_openai_headers_missing_are_none() {
        let headers = reqwest::header::HeaderMap::new();
        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert!(rl.requests_limit.is_none());
        assert!(rl.requests_remaining.is_none());
        assert!(rl.requests_reset.is_none());
        assert!(rl.tokens_limit.is_none());
        assert!(rl.tokens_remaining.is_none());
        assert!(rl.tokens_reset.is_none());
        assert!(rl.retry_after.is_none());
    }

    #[test]
    fn inject_anthropic_headers_sets_values() {
        let rl = RateLimitHeaders {
            requests_limit: Some("100".into()),
            tokens_remaining: Some("39500".into()),
            retry_after: Some("3".into()),
            ..Default::default()
        };
        let mut map = HeaderMap::new();
        rl.inject_anthropic_headers(&mut map);

        assert_eq!(
            map.get("anthropic-ratelimit-requests-limit").unwrap(),
            "100"
        );
        assert_eq!(
            map.get("anthropic-ratelimit-tokens-remaining").unwrap(),
            "39500"
        );
        assert_eq!(map.get("retry-after").unwrap(), "3");
        // Fields that were None should not be present
        assert!(map.get("anthropic-ratelimit-requests-remaining").is_none());
        assert!(map.get("anthropic-ratelimit-tokens-limit").is_none());
    }

    #[test]
    fn inject_anthropic_headers_empty_is_noop() {
        let rl = RateLimitHeaders::default();
        let mut map = HeaderMap::new();
        rl.inject_anthropic_headers(&mut map);
        assert!(map.is_empty());
    }
}
