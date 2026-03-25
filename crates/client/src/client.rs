//! High-level async client: Anthropic request in, Anthropic response out.
//!
//! Combines translation, HTTP, retry, and SSE streaming into a single ergonomic API.

use anyllm_translate::anthropic::messages::MessageResponse;
use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::anthropic::MessageCreateRequest;
use anyllm_translate::openai::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
};
use anyllm_translate::{mapping, translate_request, translate_response, TranslationConfig};
use futures::Stream;
use pin_project_lite::pin_project;

use crate::error::ClientError;
use crate::http::{build_http_client, HttpClientConfig};
use crate::rate_limit::RateLimitHeaders;
use crate::retry::{self, RetryableError};

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

    pub fn build(self) -> ClientConfig {
        ClientConfig {
            chat_completions_url: self.backend_url,
            auth: self.auth.unwrap_or(Auth::Bearer(String::new())),
            http: self.http.unwrap_or_default(),
            translation: self.translation.unwrap_or_default(),
        }
    }
}

/// Internal error type implementing [`RetryableError`] for the generic retry loop.
#[derive(Debug)]
enum InternalError {
    Request(reqwest::Error),
    ApiError { status: u16, body: String },
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

/// Async HTTP client for Anthropic-to-OpenAI translation.
///
/// Accepts Anthropic Messages API requests, translates to OpenAI format,
/// sends to the configured backend, and translates the response back.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    config: ClientConfig,
}

impl Client {
    /// Create a new client from configuration.
    pub fn new(config: ClientConfig) -> Self {
        let http = build_http_client(&config.http);
        Self { http, config }
    }

    /// Create from an existing reqwest client and configuration.
    /// Useful when you want to share an HTTP client across multiple instances.
    pub fn with_http_client(http: reqwest::Client, config: ClientConfig) -> Self {
        Self { http, config }
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
        let response: reqwest::Response = retry::send_with_retry::<InternalError>(
            &self.http,
            &self.config.chat_completions_url,
            &self.auth(),
            req,
            "backend",
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
        let response: reqwest::Response = retry::send_with_retry::<InternalError>(
            &self.http,
            &self.config.chat_completions_url,
            &self.auth(),
            req,
            "backend",
        )
        .await
        .map_err(ClientError::from)?;

        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        Ok((response, rate_limits))
    }
}

// -- Streaming implementation --

pin_project! {
    /// A stream that reads SSE frames from a reqwest response, translates
    /// OpenAI chunks to Anthropic StreamEvents, and yields them.
    struct SseTranslatingStream {
        #[pin]
        inner: futures::channel::mpsc::Receiver<Result<StreamEvent, ClientError>>,
    }
}

impl SseTranslatingStream {
    fn new(response: reqwest::Response, model: String) -> Self {
        let (mut tx, rx) = futures::channel::mpsc::channel(32);

        // Spawn a task to read SSE frames and translate them.
        tokio::spawn(async move {
            let mut translator = mapping::streaming_map::StreamingTranslator::new(model);
            let mut done = false;

            let result = crate::sse::read_sse_stream(
                response,
                |json_str| {
                    if json_str == "[DONE]" {
                        done = true;
                        return Some(translator.finish());
                    }
                    match serde_json::from_str::<ChatCompletionChunk>(json_str) {
                        Ok(chunk) => Some(translator.process_chunk(&chunk)),
                        Err(e) => {
                            tracing::debug!("failed to parse streaming chunk: {e}");
                            None
                        }
                    }
                },
                |events| {
                    for event in events {
                        // Block on send; if receiver is dropped, stop.
                        if tx.try_send(Ok(event.clone())).is_err() {
                            return false;
                        }
                    }
                    true
                },
            )
            .await;

            if let Err(e) = result {
                let _ = tx.try_send(Err(ClientError::Sse(e)));
            } else if !done {
                // Stream ended without [DONE]; flush remaining events.
                let events = translator.finish();
                for event in events {
                    if tx.try_send(Ok(event)).is_err() {
                        break;
                    }
                }
            }
        });

        Self { inner: rx }
    }
}

impl Stream for SseTranslatingStream {
    type Item = Result<StreamEvent, ClientError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
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
}
