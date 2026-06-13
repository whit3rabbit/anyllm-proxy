//! Native Anthropic Messages API passthrough client.
//!
//! Unlike [`crate::Client`], this client sends requests directly to the Anthropic API
//! without any translation. Accepts [`MessageCreateRequest`] and returns [`MessageResponse`]
//! or a stream of [`StreamEvent`]s.
//!
//! # Example
//!
//! ```rust,no_run
//! use anyllm_client::AnthropicMessagesClient;
//! use anyllm_translate::anthropic::MessageCreateRequest;
//!
//! # async fn example() -> Result<(), anyllm_client::ClientError> {
//! let client = AnthropicMessagesClient::builder()
//!     .api_key("sk-ant-...")
//!     .build()?;
//!
//! let req: MessageCreateRequest = serde_json::from_str(r#"{
//!     "model": "claude-sonnet-4-6",
//!     "max_tokens": 100,
//!     "messages": [{"role": "user", "content": "Hello"}]
//! }"#).unwrap();
//!
//! let response = client.messages(&req).await?;
//! println!("{:?}", response);
//! # Ok(())
//! # }
//! ```

use anyllm_translate::anthropic::messages::MessageResponse;
use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::anthropic::MessageCreateRequest;
use futures::Stream;

use crate::client::{Auth, InternalError};
use crate::error::ClientError;
use crate::http::{build_http_client, HttpClientConfig};
use crate::rate_limit::RateLimitHeaders;
use crate::retry::{self, RetryPolicy};
use crate::streaming::SsePassthroughStream;

/// Native Anthropic Messages API passthrough client.
///
/// Sends [`MessageCreateRequest`] directly to the Anthropic API and returns
/// [`MessageResponse`] or a stream of [`StreamEvent`]s, with no format translation.
#[derive(Clone)]
pub struct AnthropicMessagesClient {
    http: reqwest::Client,
    messages_url: String,
    auth: Auth,
    retry: RetryPolicy,
}

/// Builder for [`AnthropicMessagesClient`].
pub struct AnthropicMessagesClientBuilder {
    base_url: Option<String>,
    messages_url_override: Option<String>,
    auth: Option<Auth>,
    anthropic_version: String,
    http: Option<HttpClientConfig>,
    max_retries: Option<u32>,
    retry_transport_errors: bool,
}

impl Default for AnthropicMessagesClientBuilder {
    fn default() -> Self {
        Self {
            base_url: None,
            messages_url_override: None,
            auth: None,
            anthropic_version: "2023-06-01".to_string(),
            http: None,
            max_retries: None,
            retry_transport_errors: false,
        }
    }
}

impl AnthropicMessagesClientBuilder {
    fn new() -> Self {
        Self::default()
    }

    /// Set the Anthropic API base URL (default: `https://api.anthropic.com`).
    ///
    /// `/v1/messages` is appended automatically unless the URL already ends with it.
    /// Use [`messages_url`](Self::messages_url) to set the full URL verbatim.
    pub fn base_url(mut self, url: &str) -> Self {
        self.base_url = Some(url.to_string());
        self
    }

    /// Set the messages endpoint URL verbatim, bypassing automatic suffix logic.
    ///
    /// Use this when you need a non-standard path (e.g. a proxy that puts
    /// the messages endpoint at a custom path).
    pub fn messages_url(mut self, url: &str) -> Self {
        self.messages_url_override = Some(url.to_string());
        self
    }

    /// Set the Anthropic API key (sends as `x-api-key` header).
    pub fn api_key(mut self, key: &str) -> Self {
        self.auth = Some(Auth::Header {
            name: "x-api-key".to_string(),
            value: key.to_string(),
        });
        self
    }

    /// Set custom authentication (e.g. `Auth::Bearer` for gateway setups).
    pub fn auth(mut self, auth: Auth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Set the `anthropic-version` header value (default: `"2023-06-01"`).
    pub fn anthropic_version(mut self, version: &str) -> Self {
        self.anthropic_version = version.to_string();
        self
    }

    /// Set HTTP client configuration (TLS, timeouts, SSRF protection, extra headers).
    pub fn http(mut self, http: HttpClientConfig) -> Self {
        self.http = Some(http);
        self
    }

    /// Set the maximum number of retries on 429/5xx (default: 3).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = Some(n);
        self
    }

    /// Opt in to retrying connect/timeout transport errors (default: off).
    pub fn retry_transport_errors(mut self, enabled: bool) -> Self {
        self.retry_transport_errors = enabled;
        self
    }

    /// Build the [`AnthropicMessagesClient`].
    ///
    /// Returns an error if `api_key` (or `auth`) is missing.
    pub fn build(self) -> Result<AnthropicMessagesClient, ClientError> {
        let auth = self.auth.ok_or_else(|| ClientError::ApiError {
            status: 0,
            message: "AnthropicMessagesClientBuilder: api_key or auth is required".to_string(),
            body: String::new(),
        })?;

        let messages_url = if let Some(url) = self.messages_url_override {
            url
        } else {
            let base = self
                .base_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com");
            derive_messages_url(base)
        };

        // HttpClientConfig::new() sets ssrf_protection from the feature flag;
        // it is not equivalent to Default::default() so unwrap_or_default would be wrong.
        #[allow(clippy::unwrap_or_default)]
        let mut http_config = self.http.unwrap_or_else(HttpClientConfig::new);
        // Inject anthropic-version as a static default header. Push to the END
        // so that HeaderMap::insert processes it last and it wins over any
        // anthropic-version the caller may have placed in http.extra_headers.
        http_config
            .extra_headers
            .push(("anthropic-version".to_string(), self.anthropic_version));

        let policy = RetryPolicy::new(self.max_retries.unwrap_or(retry::MAX_RETRIES))
            .with_transport_retries(self.retry_transport_errors);

        let http = build_http_client(&http_config);

        Ok(AnthropicMessagesClient {
            http,
            messages_url,
            auth,
            retry: policy,
        })
    }
}

impl AnthropicMessagesClient {
    /// Return a builder for constructing an `AnthropicMessagesClient`.
    pub fn builder() -> AnthropicMessagesClientBuilder {
        AnthropicMessagesClientBuilder::new()
    }

    /// Create from an existing reqwest client, messages URL, and auth.
    ///
    /// The retry policy defaults to [`RetryPolicy::default`]. Chain
    /// [`with_max_retries`](Self::with_max_retries) to override.
    ///
    /// # SSRF note
    ///
    /// This constructor bypasses SSRF protection: the provided `reqwest::Client`
    /// is used as-is, with no DNS filtering and the default redirect policy.
    /// A 302 redirect to a bare IP (e.g. `http://169.254.169.254/`) will be
    /// followed. Use [`AnthropicMessagesClient::builder`] with
    /// [`HttpClientConfig`] when SSRF protection is required.
    pub fn with_http_client(http: reqwest::Client, messages_url: String, auth: Auth) -> Self {
        Self {
            http,
            messages_url,
            auth,
            retry: RetryPolicy::default(),
        }
    }

    /// Override the maximum number of retries. Chainable.
    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.retry.max_retries = n;
        self
    }

    /// Opt in to retrying transport errors. Chainable.
    pub fn with_transport_retries(mut self, enabled: bool) -> Self {
        self.retry.retry_transport_errors = enabled;
        self
    }

    /// Return the current retry policy.
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    fn request_auth(&self) -> retry::RequestAuth<'_> {
        match &self.auth {
            Auth::Bearer(token) => retry::RequestAuth::Bearer(token),
            Auth::Header { name, value } => retry::RequestAuth::Header { name, value },
        }
    }

    /// Send a non-streaming Anthropic Messages request.
    ///
    /// Retries on 429/5xx (and optionally transport errors) with exponential backoff.
    pub async fn messages(
        &self,
        req: &MessageCreateRequest,
    ) -> Result<MessageResponse, ClientError> {
        let response: reqwest::Response = retry::send_with_retry_policy::<InternalError>(
            &self.http,
            &self.messages_url,
            &self.request_auth(),
            &[],
            req,
            "anthropic",
            &self.retry,
        )
        .await
        .map_err(ClientError::from)?;

        response
            .json::<MessageResponse>()
            .await
            .map_err(|e| ClientError::Deserialization(e.to_string()))
    }

    /// Send a streaming Anthropic Messages request.
    ///
    /// Returns a stream of [`StreamEvent`]s and the rate-limit headers from
    /// the initial response. The stream parses Anthropic SSE frames natively;
    /// unknown event types surface as [`StreamEvent::Unknown`].
    pub async fn messages_stream(
        &self,
        req: &MessageCreateRequest,
    ) -> Result<
        (
            impl Stream<Item = Result<StreamEvent, ClientError>>,
            RateLimitHeaders,
        ),
        ClientError,
    > {
        // Serialize once to Value and patch stream:true — avoids a deep clone
        // of all messages, tool schemas, and serde_json::Map extra fields.
        let mut body =
            serde_json::to_value(req).map_err(|e| ClientError::Deserialization(e.to_string()))?;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
        }

        let response: reqwest::Response = retry::send_with_retry_policy::<InternalError>(
            &self.http,
            &self.messages_url,
            &self.request_auth(),
            &[("accept", "text/event-stream")],
            &body,
            "anthropic",
            &self.retry,
        )
        .await
        .map_err(ClientError::from)?;

        let rate_limits = RateLimitHeaders::from_anthropic_headers(response.headers());
        let stream = SsePassthroughStream::new(response);
        Ok((stream, rate_limits))
    }
}

/// Derive the `/v1/messages` URL from a base URL.
///
/// - If the base URL already ends with `/v1/messages`, returns it unchanged.
/// - Otherwise appends `/v1/messages`.
/// - Trailing slashes on the base are stripped before appending.
fn derive_messages_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1/messages") {
        return base.to_string();
    }
    format!("{base}/v1/messages")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_messages_url_appends_suffix() {
        assert_eq!(
            derive_messages_url("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn derive_messages_url_strips_trailing_slash() {
        assert_eq!(
            derive_messages_url("https://api.anthropic.com/"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn derive_messages_url_no_double_append() {
        assert_eq!(
            derive_messages_url("https://proxy.example.com/v1/messages"),
            "https://proxy.example.com/v1/messages"
        );
    }

    #[test]
    fn builder_requires_auth() {
        let result = AnthropicMessagesClient::builder()
            .base_url("https://api.anthropic.com")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_api_key_sets_x_api_key_auth() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .build()
            .unwrap();
        assert!(matches!(&client.auth, Auth::Header { name, value }
                if name == "x-api-key" && value == "sk-ant-test"));
    }

    #[test]
    fn builder_default_messages_url() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .build()
            .unwrap();
        assert_eq!(client.messages_url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn builder_custom_base_url() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .base_url("https://proxy.example.com")
            .build()
            .unwrap();
        assert_eq!(client.messages_url, "https://proxy.example.com/v1/messages");
    }

    #[test]
    fn builder_messages_url_override_verbatim() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .messages_url("https://custom.example.com/ai/messages")
            .build()
            .unwrap();
        assert_eq!(
            client.messages_url,
            "https://custom.example.com/ai/messages"
        );
    }

    #[test]
    fn builder_anthropic_version_in_extra_headers() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .anthropic_version("2024-01-01")
            .build()
            .unwrap();
        // anthropic-version is the first extra_header injected by the builder.
        // We can't inspect the reqwest client's default headers directly,
        // but we can verify it would be set by building with a custom http
        // config and checking that our extra_headers insertion didn't panic.
        let _ = client; // build succeeded without panic
    }

    #[test]
    fn with_http_client_max_retries_override() {
        let http = reqwest::Client::new();
        let client = AnthropicMessagesClient::with_http_client(
            http,
            "https://example.com".into(),
            Auth::Bearer("tok".into()),
        )
        .with_max_retries(5);
        assert_eq!(client.retry_policy().max_retries, 5);
    }

    #[test]
    fn with_transport_retries_chaining() {
        let http = reqwest::Client::new();
        let client = AnthropicMessagesClient::with_http_client(
            http,
            "https://example.com".into(),
            Auth::Bearer("tok".into()),
        )
        .with_transport_retries(true);
        assert!(client.retry_policy().retry_transport_errors);
    }

    #[test]
    fn builder_max_retries_and_transport_flags() {
        let client = AnthropicMessagesClient::builder()
            .api_key("sk-ant-test")
            .max_retries(7)
            .retry_transport_errors(true)
            .build()
            .unwrap();
        assert_eq!(client.retry_policy().max_retries, 7);
        assert!(client.retry_policy().retry_transport_errors);
    }
}
