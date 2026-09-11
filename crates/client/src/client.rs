//! High-level async client: Anthropic request in, Anthropic response out.
//!
//! Combines translation, HTTP, retry, and SSE streaming into a single ergonomic API.

use anyllm_translate::anthropic::messages::MessageResponse;
use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::anthropic::MessageCreateRequest;
use anyllm_translate::openai::{ChatCompletionRequest, ChatCompletionResponse};
use anyllm_translate::{translate_request, translate_response, TranslationConfig};
use futures::Stream;
use std::sync::Arc;

use crate::callback::{CallbackContext, PricingConfig, SuccessCallback};
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
#[derive(Clone)]
pub struct ClientConfig {
    /// URL for the chat completions endpoint (e.g., `https://api.openai.com/v1/chat/completions`).
    pub chat_completions_url: String,
    /// Authentication credentials.
    pub auth: Auth,
    /// HTTP client configuration (TLS, timeouts, SSRF protection).
    pub http: HttpClientConfig,
    /// Translation configuration (model mapping, lossy behavior).
    pub translation: TranslationConfig,
    /// Optional [`SuccessCallback`] fired after each successful completion.
    pub success_callback: Option<SuccessCallback>,
    /// Optional pricing table used to populate `cost_usd` on the callback payload.
    pub pricing: PricingConfig,
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("chat_completions_url", &self.chat_completions_url)
            .field("auth", &self.auth)
            .field("http", &self.http)
            .field("translation", &self.translation)
            .field(
                "success_callback",
                &self.success_callback.as_ref().map(|_| "<fn>"),
            )
            .field("pricing", &self.pricing)
            .finish()
    }
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
    success_callback: Option<SuccessCallback>,
    pricing: PricingConfig,
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

    /// Install a [`SuccessCallback`] fired after each successful completion.
    ///
    /// The callback runs synchronously on the calling task before `messages()`
    /// returns. Panics inside the closure are caught and logged; they do not
    /// propagate to the caller.
    ///
    /// Pass an `Arc<...>` to share the same callback across multiple clients
    /// or to install a runtime-mutable callback (see
    /// [`Client::set_success_callback`]).
    pub fn success_callback(mut self, cb: SuccessCallback) -> Self {
        self.success_callback = Some(cb);
        self
    }

    /// Install the pricing table used to populate `cost_usd` on the callback
    /// payload. Replaces any previously configured pricing.
    pub fn pricing(mut self, pricing: PricingConfig) -> Self {
        self.pricing = pricing;
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
            success_callback: self.success_callback,
            pricing: self.pricing,
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
            success_callback: None,
            pricing: PricingConfig::default(),
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
    callbacks: Arc<CallbackContext>,
}

impl Client {
    /// Create a new client from configuration.
    pub fn new(config: ClientConfig) -> Self {
        let http = build_http_client(&config.http);
        let callbacks = Arc::new(CallbackContext {
            callback: config.success_callback.clone(),
            pricing: config.pricing.clone(),
            warned_missing_pricing: std::sync::Mutex::new(Default::default()),
        });
        Self {
            http,
            config,
            retry: RetryPolicy::default(),
            callbacks,
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
        let callbacks = Arc::new(CallbackContext {
            callback: config.success_callback.clone(),
            pricing: config.pricing.clone(),
            warned_missing_pricing: std::sync::Mutex::new(Default::default()),
        });
        Self {
            http,
            config,
            retry: RetryPolicy::default(),
            callbacks,
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

    /// Install or replace the [`SuccessCallback`] for this client.
    ///
    /// Takes `&mut self` because the callback is stored inside an
    /// `Arc<CallbackContext>` shared across `Client` clones — runtime mutation
    /// of a shared Arc requires unique ownership. For shared-client scenarios
    /// (where `Client` is cloned), build the callback into `ClientConfig`
    /// before cloning.
    pub fn set_success_callback(&mut self, cb: Option<SuccessCallback>) {
        // `Arc::get_mut` succeeds only when this `Client` is the sole owner of
        // its `Arc<CallbackContext>`. If any clone exists, refcount > 1 and
        // `get_mut` returns None — we silently keep the old callback in that
        // case. Logging here would be too noisy for a hot-path SDK.
        if let Some(ctx_mut) = Arc::get_mut(&mut self.callbacks) {
            ctx_mut.callback = cb;
        }
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
    ///
    /// If a [`SuccessCallback`] was registered, it fires exactly once on
    /// successful completion with the original Anthropic request, the translated
    /// Anthropic response, the resolved backend model, the wall-clock duration,
    /// and (when pricing is configured) the computed `cost_usd`. The callback
    /// does NOT fire on transport errors or non-2xx API errors.
    pub async fn messages(
        &self,
        req: &MessageCreateRequest,
    ) -> Result<MessageResponse, ClientError> {
        let started = std::time::Instant::now();
        let openai_req = translate_request(req, &self.config.translation)?;
        let (resp, _status, _rate_limits) = self.chat_completion(&openai_req).await?;
        let anthropic_resp = translate_response(&resp, &req.model);

        // Fire the success callback if one is configured. The callback runs
        // synchronously on this task before `messages()` returns. Panics inside
        // the callback are caught by `CallbackContext::fire`.
        let input = crate::callback::CallbackInput {
            request: req.clone(),
            response: anthropic_resp.clone(),
            request_model: req.model.clone(),
            backend_model: openai_req.model.clone(),
            duration: started.elapsed(),
            cost_usd: self.callbacks.cost_for(
                &openai_req.model,
                u64::from(anthropic_resp.usage.input_tokens),
                u64::from(anthropic_resp.usage.output_tokens),
            ),
        };
        self.callbacks.fire(input);

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
mod tests;
