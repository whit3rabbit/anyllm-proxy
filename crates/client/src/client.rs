//! High-level async client: Anthropic request in, Anthropic response out.
//!
//! Combines translation, HTTP, retry, and SSE streaming into a single ergonomic API.

use anyllm_translate::anthropic::messages::MessageResponse;
use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::anthropic::MessageCreateRequest;
use anyllm_translate::openai::{ChatCompletionRequest, ChatCompletionResponse};
use anyllm_translate::{translate_request, translate_response, TranslationConfig};
use futures::Stream;

use crate::error::ClientError;
use crate::http::{build_http_client, HttpClientConfig};
use crate::rate_limit::RateLimitHeaders;
use crate::retry::{self, RetryPolicy, RetryableError};
use crate::streaming::SseTranslatingStream;

/// Authentication for the backend API.
#[derive(Clone, Debug)]
pub enum Auth {
    /// Bearer token (e.g., OpenAI API key).
    Bearer(String),
    /// Custom header (e.g., `x-goog-api-key` for Google).
    Header { name: String, value: String },
}

/// Configuration for the [`Client`].
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// URL for the chat completions endpoint (e.g., `https://api.openai.com/v1/chat/completions`).
    pub chat_completions_url: String,
    /// Authentication credentials.
    pub auth: Auth,
    /// HTTP client configuration (TLS, timeouts, SSRF protection).
    pub http: HttpClientConfig,
    /// Translation configuration (model mapping, lossy behavior).
    pub translation: TranslationConfig,
}

impl ClientConfig {
    /// Return a builder for constructing a `ClientConfig` with method chaining.
    pub fn builder() -> ClientConfigBuilder {
        ClientConfigBuilder::default()
    }
}

/// Builder for [`ClientConfig`].
#[derive(Default)]
pub struct ClientConfigBuilder {
    backend_url: String,
    auth: Option<Auth>,
    http: Option<HttpClientConfig>,
    translation: Option<TranslationConfig>,
}

impl ClientConfigBuilder {
    /// Set the chat completions endpoint URL.
    /// For OpenAI: `https://api.openai.com/v1/chat/completions`
    pub fn backend_url(mut self, url: impl Into<String>) -> Self {
        self.backend_url = url.into();
        self
    }

    /// Set authentication credentials.
    pub fn auth(mut self, auth: Auth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Set HTTP client configuration. Uses secure defaults if not specified.
    pub fn http(mut self, http: HttpClientConfig) -> Self {
        self.http = Some(http);
        self
    }

    /// Set translation configuration.
    pub fn translation(mut self, translation: TranslationConfig) -> Self {
        self.translation = Some(translation);
        self
    }

    /// Finalize the builder into a `ClientConfig`. Missing fields use secure defaults.
    pub fn build(self) -> ClientConfig {
        ClientConfig {
            chat_completions_url: self.backend_url,
            auth: self.auth.unwrap_or_else(|| {
                tracing::warn!(
                    "ClientConfig built without auth credentials; \
                     requests will be sent with an empty Bearer token"
                );
                Auth::Bearer(String::new())
            }),
            http: self.http.unwrap_or_default(),
            translation: self.translation.unwrap_or_default(),
        }
    }
}

/// Internal error type implementing [`RetryableError`] for the generic retry loop.
pub(crate) enum InternalError {
    Request(reqwest::Error),
    ApiError { status: u16, body: String },
}

impl std::fmt::Debug for InternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "InternalError::Request({e})"),
            Self::ApiError { status, body } => {
                write!(
                    f,
                    "InternalError::ApiError {{ status: {status}, body: {body:?} }}"
                )
            }
        }
    }
}

impl RetryableError for InternalError {
    fn from_request(e: reqwest::Error) -> Self {
        Self::Request(e)
    }
    fn from_api_response(status: u16, body: &str) -> Self {
        Self::ApiError {
            status,
            body: body.to_string(),
        }
    }
}

impl From<InternalError> for ClientError {
    fn from(e: InternalError) -> Self {
        match e {
            InternalError::Request(e) => ClientError::Transport(e),
            InternalError::ApiError { status, body } => ClientError::ApiError {
                status,
                message: format!("Backend returned status {status}"),
                body,
            },
        }
    }
}

/// Simplified builder for [`Client`] with sensible defaults.
///
/// Use this when you want a quick client without manually wiring
/// [`ClientConfig`], [`HttpClientConfig`], and [`TranslationConfig`].
///
/// # Examples
///
/// ```rust,no_run
/// use anyllm_client::ClientBuilder;
///
/// # fn example() -> Result<(), anyllm_client::ClientError> {
/// let client = ClientBuilder::new()
///     .base_url("https://api.openai.com/v1/chat/completions")
///     .api_key("sk-...")
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct ClientBuilder {
    base_url: Option<String>,
    api_key: Option<String>,
    connect_timeout: Option<std::time::Duration>,
    request_timeout: Option<std::time::Duration>,
    read_timeout: Option<std::time::Duration>,
    max_retries: Option<u32>,
    retry_transport_errors: bool,
    extra_headers: Vec<(String, String)>,
}

impl ClientBuilder {
    /// Create a new builder with all fields unset.
    pub fn new() -> Self {
        Self {
            base_url: None,
            api_key: None,
            connect_timeout: None,
            request_timeout: None,
            read_timeout: None,
            max_retries: None,
            retry_transport_errors: false,
            extra_headers: Vec::new(),
        }
    }

    /// Set the backend URL (e.g., `https://api.openai.com/v1/chat/completions`).
    pub fn base_url(mut self, url: &str) -> Self {
        self.base_url = Some(url.to_string());
        self
    }

    /// Set the API key used as a Bearer token.
    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    /// Set the TCP connection timeout (default: 10s).
    pub fn connect_timeout(mut self, duration: std::time::Duration) -> Self {
        self.connect_timeout = Some(duration);
        self
    }

    /// Set the total request timeout — wall-clock limit from first byte sent to
    /// last byte received. Unset by default; [`read_timeout`](Self::read_timeout)
    /// already caps slow streaming responses.
    pub fn timeout(mut self, duration: std::time::Duration) -> Self {
        self.request_timeout = Some(duration);
        self
    }

    /// Set the read timeout (default: 900s).
    pub fn read_timeout(mut self, duration: std::time::Duration) -> Self {
        self.read_timeout = Some(duration);
        self
    }

    /// Set the maximum number of retries on 429/5xx (default: 3).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = Some(n);
        self
    }

    /// Opt in to retrying connect/timeout transport errors (default: off).
    ///
    /// See [`RetryPolicy::retry_transport_errors`].
    pub fn retry_transport_errors(mut self, enabled: bool) -> Self {
        self.retry_transport_errors = enabled;
        self
    }

    /// Add a static header sent on every request (e.g. `HTTP-Referer` for OpenRouter).
    ///
    /// May be called multiple times; headers are applied in order.
    /// Invalid header names or values are skipped with a warning.
    pub fn extra_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    /// Build the [`Client`], returning an error if `base_url` is missing or `api_key` is empty.
    pub fn build(self) -> Result<Client, ClientError> {
        let base_url = self.base_url.ok_or_else(|| ClientError::ApiError {
            status: 0,
            message: "ClientBuilder: base_url is required".to_string(),
            body: String::new(),
        })?;

        if let Some(ref key) = self.api_key {
            if key.is_empty() {
                return Err(ClientError::ApiError {
                    status: 0,
                    message: "ClientBuilder: api_key is empty".to_string(),
                    body: String::new(),
                });
            }
        }

        let http_config = HttpClientConfig {
            connect_timeout: self.connect_timeout,
            request_timeout: self.request_timeout,
            read_timeout: self.read_timeout,
            extra_headers: self.extra_headers,
            ..HttpClientConfig::new()
        };

        let policy = RetryPolicy::new(self.max_retries.unwrap_or(retry::MAX_RETRIES))
            .with_transport_retries(self.retry_transport_errors);

        let config = ClientConfig {
            chat_completions_url: base_url,
            auth: Auth::Bearer(self.api_key.unwrap_or_default()),
            http: http_config,
            translation: TranslationConfig::default(),
        };

        let mut client = Client::new(config);
        client.retry = policy;
        Ok(client)
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async HTTP client for Anthropic-to-OpenAI translation.
///
/// Accepts Anthropic Messages API requests, translates to OpenAI format,
/// sends to the configured backend, and translates the response back.
///
/// # Examples
///
/// ```rust,no_run
/// use anyllm_client::{Client, ClientConfig, Auth};
///
/// let config = ClientConfig::builder()
///     .backend_url("https://api.openai.com/v1/chat/completions")
///     .auth(Auth::Bearer("sk-...".into()))
///     .build();
/// let client = Client::new(config);
/// ```
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    config: ClientConfig,
    retry: RetryPolicy,
}

impl Client {
    /// Create a new client from configuration.
    pub fn new(config: ClientConfig) -> Self {
        let http = build_http_client(&config.http);
        Self {
            http,
            config,
            retry: RetryPolicy::default(),
        }
    }

    /// Return a [`ClientBuilder`] for simplified construction.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use anyllm_client::Client;
    ///
    /// # fn example() -> Result<(), anyllm_client::ClientError> {
    /// let client = Client::builder()
    ///     .base_url("https://api.openai.com/v1/chat/completions")
    ///     .api_key("sk-...")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create from an existing reqwest client and configuration.
    /// Useful when you want to share an HTTP client across multiple instances.
    ///
    /// The retry policy defaults to [`RetryPolicy::default`] (3 retries, no transport retry).
    /// Call [`with_max_retries`](Self::with_max_retries) or
    /// [`with_transport_retries`](Self::with_transport_retries) after construction to override.
    pub fn with_http_client(http: reqwest::Client, config: ClientConfig) -> Self {
        Self {
            http,
            config,
            retry: RetryPolicy::default(),
        }
    }

    /// Override the maximum number of retries on 429/5xx. Chainable.
    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.retry.max_retries = n;
        self
    }

    /// Opt in to retrying connect/timeout transport errors. Chainable.
    pub fn with_transport_retries(mut self, enabled: bool) -> Self {
        self.retry.retry_transport_errors = enabled;
        self
    }

    /// Return the current retry policy.
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    fn auth(&self) -> retry::RequestAuth<'_> {
        match &self.config.auth {
            Auth::Bearer(token) => retry::RequestAuth::Bearer(token),
            Auth::Header { name, value } => retry::RequestAuth::Header { name, value },
        }
    }

    /// Send an Anthropic Messages API request and get an Anthropic response.
    ///
    /// Translates the request to OpenAI format, sends it, and translates the
    /// response back. Retries on 429/5xx with exponential backoff.
    pub async fn messages(
        &self,
        req: &MessageCreateRequest,
    ) -> Result<MessageResponse, ClientError> {
        let openai_req = translate_request(req, &self.config.translation)?;
        let (resp, _status, _rate_limits) = self.chat_completion(&openai_req).await?;
        let anthropic_resp = translate_response(&resp, &req.model);
        Ok(anthropic_resp)
    }

    /// Send an Anthropic Messages API request and get a stream of Anthropic SSE events.
    ///
    /// The returned stream yields `StreamEvent` items. Translation happens
    /// incrementally as chunks arrive from the backend.
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
        let mut openai_req = translate_request(req, &self.config.translation)?;
        openai_req.stream = Some(true);
        let (response, rate_limits) = self.chat_completion_stream_raw(&openai_req).await?;

        let model = req.model.clone();
        let stream = SseTranslatingStream::new(response, model);
        Ok((stream, rate_limits))
    }

    /// Send a pre-translated OpenAI Chat Completion request.
    ///
    /// Useful when you want to handle translation yourself and just need the
    /// HTTP client with retry logic.
    pub async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<(ChatCompletionResponse, u16, RateLimitHeaders), ClientError> {
        let response: reqwest::Response = retry::send_with_retry_policy::<InternalError>(
            &self.http,
            &self.config.chat_completions_url,
            &self.auth(),
            &[],
            req,
            "backend",
            &self.retry,
        )
        .await
        .map_err(ClientError::from)?;

        let status = response.status().as_u16();
        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        let body = response
            .json::<ChatCompletionResponse>()
            .await
            .map_err(|e| ClientError::Deserialization(e.to_string()))?;
        Ok((body, status, rate_limits))
    }

    /// Send a streaming Chat Completion request and get the raw response.
    async fn chat_completion_stream_raw(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<(reqwest::Response, RateLimitHeaders), ClientError> {
        let response: reqwest::Response = retry::send_with_retry_policy::<InternalError>(
            &self.http,
            &self.config.chat_completions_url,
            &self.auth(),
            &[],
            req,
            "backend",
            &self.retry,
        )
        .await
        .map_err(ClientError::from)?;

        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        Ok((response, rate_limits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_builder_defaults() {
        let config = ClientConfig::builder()
            .backend_url("https://api.openai.com/v1/chat/completions")
            .auth(Auth::Bearer("sk-test".into()))
            .build();

        assert_eq!(
            config.chat_completions_url,
            "https://api.openai.com/v1/chat/completions"
        );
        assert!(matches!(config.auth, Auth::Bearer(ref s) if s == "sk-test"));
    }

    #[test]
    fn client_config_builder_with_translation() {
        let translation = TranslationConfig::builder()
            .model_map("haiku", "gpt-4o-mini")
            .model_map("sonnet", "gpt-4o")
            .build();

        let config = ClientConfig::builder()
            .backend_url("https://api.openai.com/v1/chat/completions")
            .auth(Auth::Bearer("sk-test".into()))
            .translation(translation)
            .build();

        assert!(config.translation.map_model("claude-3-haiku").is_ok());
    }

    #[test]
    fn client_creates_without_panic() {
        let config = ClientConfig::builder()
            .backend_url("https://api.openai.com/v1/chat/completions")
            .auth(Auth::Bearer("sk-test".into()))
            .http(HttpClientConfig {
                ssrf_protection: false,
                ..Default::default()
            })
            .build();

        let _client = Client::new(config);
    }

    #[test]
    fn client_builder_success() {
        let client = ClientBuilder::new()
            .base_url("https://api.openai.com/v1/chat/completions")
            .api_key("sk-test")
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(120))
            .read_timeout(std::time::Duration::from_secs(30))
            .max_retries(2)
            .build();
        assert!(client.is_ok());
    }

    #[test]
    fn client_builder_empty_api_key_rejected() {
        let result = ClientBuilder::new()
            .base_url("https://api.openai.com/v1/chat/completions")
            .api_key("")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn client_builder_max_retries_stored() {
        let client = ClientBuilder::new()
            .base_url("https://api.openai.com/v1/chat/completions")
            .max_retries(7)
            .build()
            .unwrap();
        assert_eq!(client.retry_policy().max_retries, 7);
    }

    #[test]
    fn client_builder_missing_url() {
        let result = ClientBuilder::new().api_key("sk-test").build();
        assert!(result.is_err());
    }

    #[test]
    fn client_builder_default_api_key() {
        // No api_key set: should still build (empty bearer token).
        let client = ClientBuilder::new().base_url("https://example.com").build();
        assert!(client.is_ok());
    }

    #[test]
    fn client_builder_via_client() {
        let client = Client::builder().base_url("https://example.com").build();
        assert!(client.is_ok());
    }

    #[test]
    fn client_builder_default_trait() {
        let builder = ClientBuilder::default();
        assert!(builder.base_url.is_none());
    }

    #[test]
    fn with_http_client_max_retries_override() {
        let config = ClientConfig::builder()
            .backend_url("https://example.com")
            .auth(Auth::Bearer("sk-test".into()))
            .http(HttpClientConfig {
                ssrf_protection: false,
                ..Default::default()
            })
            .build();
        let http = build_http_client(&config.http);
        let client = Client::with_http_client(http, config).with_max_retries(7);
        assert_eq!(client.retry_policy().max_retries, 7);
    }

    #[test]
    fn with_transport_retries_chaining() {
        let client = Client::builder()
            .base_url("https://example.com")
            .build()
            .unwrap()
            .with_transport_retries(true);
        assert!(client.retry_policy().retry_transport_errors);
    }

    #[test]
    fn client_builder_transport_retries_flag() {
        let client = ClientBuilder::new()
            .base_url("https://example.com")
            .retry_transport_errors(true)
            .build()
            .unwrap();
        assert!(client.retry_policy().retry_transport_errors);
    }

    #[test]
    fn client_builder_extra_header() {
        // Just verify it builds; header arrival on wire is tested in integration tests.
        let client = ClientBuilder::new()
            .base_url("https://example.com")
            .extra_header("HTTP-Referer", "https://myapp.com")
            .extra_header("X-Title", "My App")
            .build()
            .unwrap();
        assert_eq!(client.config.http.extra_headers.len(), 2);
    }
}
