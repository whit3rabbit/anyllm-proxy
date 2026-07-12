// Passthrough client for forwarding requests to the real Anthropic API.
// No translation: receives Anthropic-format request bytes, returns Anthropic-format response.

use super::{build_http_client, RateLimitHeaders};
use crate::config::{BackendAuth, BackendConfig, TlsConfig};
use reqwest::Client;
use tokio::time::sleep;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AnthropicAuth {
    ApiKey(String),
    AuthToken(String),
}

impl AnthropicAuth {
    pub(crate) fn from_backend_auth(auth: &BackendAuth) -> Self {
        match auth {
            BackendAuth::AnthropicApiKey(key) => Self::ApiKey(key.clone()),
            BackendAuth::AnthropicAuthToken(token) => Self::AuthToken(token.clone()),
            BackendAuth::BearerToken(token) => {
                match BackendAuth::anthropic_from_api_key_like(token.clone()) {
                    BackendAuth::AnthropicApiKey(key) => Self::ApiKey(key),
                    BackendAuth::AnthropicAuthToken(token) => Self::AuthToken(token),
                    _ => unreachable!("anthropic_from_api_key_like only returns Anthropic auth"),
                }
            }
            BackendAuth::GoogleApiKey(key) | BackendAuth::AzureApiKey(key) => {
                Self::ApiKey(key.clone())
            }
        }
    }

    pub(crate) fn header(&self) -> (&'static str, String) {
        match self {
            Self::ApiKey(key) => ("x-api-key", key.clone()),
            Self::AuthToken(token) => ("authorization", format!("Bearer {token}")),
        }
    }
}

/// HTTP client that forwards Anthropic requests as-is to the upstream Anthropic API.
#[derive(Clone)]
pub struct AnthropicClient {
    client: Client,
    base_url: String,
    messages_url: String,
    auth: AnthropicAuth,
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
        let client = build_http_client(&bc.tls, bc.allow_local_ssrf);
        let (base_url, messages_url) = anthropic_urls(&bc.base_url);
        Self {
            client,
            base_url,
            messages_url,
            auth: AnthropicAuth::from_backend_auth(&bc.backend_auth),
        }
    }

    /// Create from raw parts (used in legacy single-backend mode).
    pub fn new(base_url: &str, auth: &BackendAuth, tls: &TlsConfig) -> Self {
        let client = build_http_client(tls, false);
        let (base_url, messages_url) = anthropic_urls(base_url);
        Self {
            client,
            base_url,
            messages_url,
            auth: AnthropicAuth::from_backend_auth(auth),
        }
    }

    #[cfg(test)]
    pub(crate) fn auth_header(&self) -> (&'static str, String) {
        self.auth.header()
    }

    /// Apply required Anthropic authentication headers.
    /// x-api-key and anthropic-version are mandatory per the Anthropic API spec;
    /// without the version header, the API rejects requests.
    ///
    /// `override_auth`, when `Some`, is forwarded verbatim (exact name+value the
    /// client sent) INSTEAD OF the operator's configured credential -- used by
    /// `ANTHROPIC_FORWARD_CLIENT_AUTH`. Exactly one of the two branches ever calls
    /// `.header()` for the credential, so no duplicate/conflicting credential
    /// header can reach upstream (reqwest's `RequestBuilder::header` APPENDS
    /// rather than replaces, so calling it twice for the same header name would
    /// send two header lines, not an override).
    fn auth_request(
        &self,
        rb: reqwest::RequestBuilder,
        override_auth: Option<(&str, &str)>,
    ) -> reqwest::RequestBuilder {
        let rb = match override_auth {
            Some((name, value)) => rb.header(name, value),
            None => {
                let (name, value) = self.auth.header();
                rb.header(name, value)
            }
        };
        rb.header("anthropic-version", "2023-06-01")
    }

    /// Forward a non-streaming request. Returns raw response body and rate limit headers.
    /// `extra_headers` are forwarded verbatim to upstream without modification.
    pub async fn forward(
        &self,
        body: bytes::Bytes,
        extra_headers: &[(&str, &str)],
        override_auth: Option<(&str, &str)>,
    ) -> Result<(bytes::Bytes, RateLimitHeaders), AnthropicClientError> {
        let response = self
            .send_with_retry(body, false, extra_headers, override_auth)
            .await?;
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
        override_auth: Option<(&str, &str)>,
    ) -> Result<(reqwest::Response, RateLimitHeaders), AnthropicClientError> {
        let response = self
            .send_with_retry(body, true, extra_headers, override_auth)
            .await?;
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
        override_auth: Option<(&str, &str)>,
    ) -> Result<reqwest::Response, AnthropicClientError> {
        let url = format!("{}{}", self.base_url, path);
        let rb = self
            .client
            .request(method, &url)
            .header("content-type", "application/json")
            .body(body);
        let rb = self.auth_request(rb, override_auth);
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
        override_auth: Option<(&str, &str)>,
    ) -> Result<reqwest::Response, AnthropicClientError> {
        let content_type = "application/json";
        for attempt in 0..=super::MAX_RETRIES {
            let rb = self
                .client
                .post(&self.messages_url)
                .header("content-type", content_type)
                .body(body.clone());
            let rb = self.auth_request(rb, override_auth);
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
                // A 429 carrying hard quota/credit exhaustion never clears by
                // waiting; surface it immediately, consistent with the shared
                // client retry loop. Reading the body also returns the connection
                // to the pool.
                if status == 429 {
                    let resp_body = response.bytes().await.unwrap_or_default();
                    if anyllm_client::retry::is_quota_exhausted(&String::from_utf8_lossy(
                        &resp_body,
                    )) {
                        tracing::warn!(
                            status,
                            "Anthropic returned quota/credit exhaustion; not retrying"
                        );
                        return Err(AnthropicClientError::ApiError {
                            status,
                            body: resp_body,
                        });
                    }
                } else {
                    // Drain body so connection returns to pool
                    drop(response.bytes().await);
                }
                tracing::warn!(
                    status,
                    attempt = attempt + 1,
                    max_retries = super::MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "retryable error from Anthropic, backing off"
                );
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

fn anthropic_urls(base_url: &str) -> (String, String) {
    let base_url = base_url.trim_end_matches('/').to_string();
    let disable_suffix = std::env::var("LITELLM_ANTHROPIC_DISABLE_URL_SUFFIX")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    if disable_suffix {
        return (base_url.clone(), base_url);
    }

    if let Some(root) = base_url.strip_suffix("/v1/messages") {
        return (root.to_string(), base_url);
    }

    let messages_url = format!("{base_url}/v1/messages");
    (base_url, messages_url)
}

#[cfg(test)]
mod tests {
    use super::{anthropic_urls, AnthropicAuth, AnthropicClient};
    use crate::config::{BackendAuth, TlsConfig};
    use std::sync::Mutex;

    fn credential_headers(rb: reqwest::RequestBuilder) -> Vec<(String, String)> {
        let req = rb.build().expect("request builds");
        req.headers()
            .iter()
            .filter(|(name, _)| {
                let n = name.as_str();
                n == "x-api-key" || n == "authorization"
            })
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    fn test_client() -> AnthropicClient {
        AnthropicClient::new(
            "https://api.anthropic.com",
            &BackendAuth::AnthropicApiKey("operator-key".to_string()),
            &TlsConfig::default(),
        )
    }

    #[test]
    fn auth_request_without_override_uses_operator_credential() {
        let client = test_client();
        let rb = client.client.post(&client.messages_url);
        let rb = client.auth_request(rb, None);
        let headers = credential_headers(rb);
        assert_eq!(
            headers,
            vec![("x-api-key".to_string(), "operator-key".to_string())]
        );
    }

    #[test]
    fn auth_request_with_x_api_key_override_replaces_operator_credential() {
        let client = test_client();
        let rb = client.client.post(&client.messages_url);
        let rb = client.auth_request(rb, Some(("x-api-key", "client-key")));
        let headers = credential_headers(rb);
        assert_eq!(
            headers,
            vec![("x-api-key".to_string(), "client-key".to_string())],
            "operator credential must not appear anywhere in the built request"
        );
    }

    #[test]
    fn auth_request_with_bearer_override_is_forwarded_unmodified() {
        let client = test_client();
        let rb = client.client.post(&client.messages_url);
        let rb = client.auth_request(rb, Some(("authorization", "Bearer sk-ant-oat-abc123")));
        let headers = credential_headers(rb);
        assert_eq!(
            headers,
            vec![(
                "authorization".to_string(),
                "Bearer sk-ant-oat-abc123".to_string()
            )],
            "must forward the Bearer token as-is, not convert it to x-api-key"
        );
    }

    #[test]
    fn auth_request_never_sends_duplicate_credential_headers() {
        // Regression guard: reqwest's RequestBuilder::header() appends rather
        // than replaces, so auth_request must never call it twice for the
        // credential (once for the operator's, once for an override) or two
        // conflicting header lines would reach upstream.
        let client = test_client();
        for override_auth in [None, Some(("x-api-key", "client-key"))] {
            let rb = client.client.post(&client.messages_url);
            let rb = client.auth_request(rb, override_auth);
            assert_eq!(credential_headers(rb).len(), 1);
        }
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn appends_messages_suffix_by_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("LITELLM_ANTHROPIC_DISABLE_URL_SUFFIX") };
        let (base, messages) = anthropic_urls("https://api.anthropic.com/");
        assert_eq!(base, "https://api.anthropic.com");
        assert_eq!(messages, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn does_not_double_append_messages_suffix() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("LITELLM_ANTHROPIC_DISABLE_URL_SUFFIX") };
        let (base, messages) = anthropic_urls("https://proxy.example/v1/messages");
        assert_eq!(base, "https://proxy.example");
        assert_eq!(messages, "https://proxy.example/v1/messages");
    }

    #[test]
    fn disable_suffix_uses_base_url_as_messages_url() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("LITELLM_ANTHROPIC_DISABLE_URL_SUFFIX", "true") };
        let (base, messages) = anthropic_urls("https://proxy.example/custom/path");
        assert_eq!(base, "https://proxy.example/custom/path");
        assert_eq!(messages, "https://proxy.example/custom/path");
        unsafe { std::env::remove_var("LITELLM_ANTHROPIC_DISABLE_URL_SUFFIX") };
    }

    #[test]
    fn api_key_auth_uses_x_api_key_header() {
        let auth = AnthropicAuth::from_backend_auth(&BackendAuth::AnthropicApiKey(
            "sk-ant-api".to_string(),
        ));
        let (name, value) = auth.header();
        assert_eq!(name, "x-api-key");
        assert_eq!(value, "sk-ant-api");
    }

    #[test]
    fn auth_token_uses_bearer_header() {
        let auth = AnthropicAuth::from_backend_auth(&BackendAuth::AnthropicAuthToken(
            "sk-ant-oat-test".to_string(),
        ));
        let (name, value) = auth.header();
        assert_eq!(name, "authorization");
        assert_eq!(value, "Bearer sk-ant-oat-test");
    }
}
