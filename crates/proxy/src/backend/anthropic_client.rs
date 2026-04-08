// Passthrough client for forwarding requests to the real Anthropic API.
// No translation: receives Anthropic-format request bytes, returns Anthropic-format response.

use super::{build_http_client, RateLimitHeaders};
use crate::config::{BackendConfig, TlsConfig};
use reqwest::Client;
use tokio::time::sleep;

/// HTTP client that forwards Anthropic requests as-is to the upstream Anthropic API.
#[derive(Clone)]
pub struct AnthropicClient {
    client: Client,
    base_url: String,
    messages_url: String,
    api_key: String,
}

/// Error type for the Anthropic passthrough client.
#[derive(Debug)]
pub enum AnthropicClientError {
    /// Transport-level error (connection, timeout, DNS).
    Transport(String),
    /// Upstream returned a non-success status. Body is raw bytes for passthrough.
    ApiError { status: u16, body: bytes::Bytes },
}

impl std::fmt::Display for AnthropicClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "Anthropic transport error: {msg}"),
            Self::ApiError { status, .. } => write!(f, "Anthropic API error (status {status})"),
        }
    }
}

impl AnthropicClient {
    /// Create from a BackendConfig (used in multi-backend mode).
    pub fn from_backend_config(bc: &BackendConfig) -> Self {
        let client = build_http_client(&bc.tls);
        let base_url = bc.base_url.trim_end_matches('/').to_string();
        let messages_url = format!("{base_url}/v1/messages");
        Self {
            client,
            base_url,
            messages_url,
            api_key: bc.api_key.clone(),
        }
    }

    /// Create from raw parts (used in legacy single-backend mode).
    pub fn new(base_url: &str, api_key: &str, tls: &TlsConfig) -> Self {
        let client = build_http_client(tls);
        let base_url = base_url.trim_end_matches('/').to_string();
        let messages_url = format!("{base_url}/v1/messages");
        Self {
            client,
            base_url,
            messages_url,
            api_key: api_key.to_string(),
        }
    }

    /// Apply required Anthropic authentication headers.
    /// x-api-key and anthropic-version are mandatory per the Anthropic API spec;
    /// without the version header, the API rejects requests.
    fn auth_request(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
    }

    /// Forward a non-streaming request. Returns raw response body and rate limit headers.
    /// `extra_headers` are forwarded verbatim to upstream without modification.
    pub async fn forward(
        &self,
        body: bytes::Bytes,
        extra_headers: &[(&str, &str)],
    ) -> Result<(bytes::Bytes, RateLimitHeaders), AnthropicClientError> {
        let response = self.send_with_retry(body, false, extra_headers).await?;
        let rate_limits = RateLimitHeaders::from_anthropic_headers(response.headers());
        let resp_body = response
            .bytes()
            .await
            .map_err(|e| AnthropicClientError::Transport(e.to_string()))?;
        Ok((resp_body, rate_limits))
    }

    /// Forward a streaming request. Returns the raw response for SSE piping.
    /// `extra_headers` are forwarded verbatim to upstream without modification.
    pub async fn forward_stream(
        &self,
        body: bytes::Bytes,
        extra_headers: &[(&str, &str)],
    ) -> Result<(reqwest::Response, RateLimitHeaders), AnthropicClientError> {
        let response = self.send_with_retry(body, true, extra_headers).await?;
        let rate_limits = RateLimitHeaders::from_anthropic_headers(response.headers());
        Ok((response, rate_limits))
    }

    /// Forward a request to an arbitrary Anthropic API path with any HTTP method.
    /// Used by the generic Anthropic passthrough to reach batch, file, and other
    /// endpoints that are not /v1/messages. No retry: batch/file ops are not safe
    /// to retry blindly.
    pub async fn forward_generic(
        &self,
        method: reqwest::Method,
        path: &str,
        body: bytes::Bytes,
        extra_headers: &[(&str, &str)],
    ) -> Result<reqwest::Response, AnthropicClientError> {
        let url = format!("{}{}", self.base_url, path);
        let rb = self
            .client
            .request(method, &url)
            .header("content-type", "application/json")
            .body(body);
        let rb = self.auth_request(rb);
        let rb = extra_headers.iter().fold(rb, |rb, &(k, v)| rb.header(k, v));
        rb.send()
            .await
            .map_err(|e| AnthropicClientError::Transport(e.to_string()))
    }

    /// Send with retry on 429/5xx. For passthrough, we retry the raw body bytes.
    async fn send_with_retry(
        &self,
        body: bytes::Bytes,
        stream: bool,
        extra_headers: &[(&str, &str)],
    ) -> Result<reqwest::Response, AnthropicClientError> {
        let content_type = "application/json";
        for attempt in 0..=super::MAX_RETRIES {
            let rb = self
                .client
                .post(&self.messages_url)
                .header("content-type", content_type)
                .body(body.clone());
            let rb = self.auth_request(rb);
            // Tell upstream we expect SSE format; the Anthropic routing layer
            // may use this hint to optimize response handling.
            let rb = if stream {
                rb.header("accept", "text/event-stream")
            } else {
                rb
            };
            let rb = extra_headers.iter().fold(rb, |rb, &(k, v)| rb.header(k, v));

            let response = rb
                .send()
                .await
                .map_err(|e| AnthropicClientError::Transport(e.to_string()))?;
            let status = response.status().as_u16();

            if (200..300).contains(&status) {
                return Ok(response);
            }

            if attempt < super::MAX_RETRIES && super::is_retryable(status) {
                let retry_after = super::parse_retry_after(response.headers());
                let delay = super::backoff_delay(attempt, retry_after);
                tracing::warn!(
                    status,
                    attempt = attempt + 1,
                    max_retries = super::MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "retryable error from Anthropic, backing off"
                );
                // Drain body so connection returns to pool
                drop(response.bytes().await);
                sleep(delay).await;
                continue;
            }

            let resp_body = response.bytes().await.unwrap_or_default();
            return Err(AnthropicClientError::ApiError {
                status,
                body: resp_body,
            });
        }
        unreachable!("loop runs MAX_RETRIES+1 times and always returns")
    }
}
