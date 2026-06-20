// reqwest client for calling OpenAI endpoints

use super::{build_http_client, RateLimitHeaders, RetryableError};
use crate::config::{BackendAuth, BackendKind, Config};
use anyllm_translate::openai::{
    self, tool_normalization::normalize_chat_completion_response_value,
};
use reqwest::Client;

/// HTTP client for OpenAI-compatible Chat Completions APIs with retry logic.
/// Works with both OpenAI and Vertex AI OpenAI-compatible endpoints.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    chat_completions_url: String,
    responses_url: String,
    embeddings_url: String,
    auth: BackendAuth,
    /// The backend kind, needed for constructing passthrough URLs at runtime.
    backend_kind: BackendKind,
    /// Raw base URL from config, used to build passthrough endpoint URLs.
    base_url: String,
    /// Provider ID when this client was built for an OpenAI-compatible stub (e.g. "zai").
    /// None for first-party OpenAI, Azure, Gemini, and Vertex backends.
    provider_id: Option<String>,
}

impl OpenAIClient {
    /// Create a new client from proxy configuration.
    /// Configures mTLS identity and custom CA cert if present in config.
    pub fn new(config: &Config) -> Self {
        let client = build_http_client(&config.tls);

        // Each provider uses a different URL structure for the same API:
        // - OpenAI: {base}/v1/chat/completions (base has no path)
        // - Vertex: {base}/chat/completions (base ends at .../openapi)
        // - Gemini: {base}/chat/completions (config appends /openai to base)
        let (chat_completions_url, responses_url, embeddings_url) = match config.backend {
            BackendKind::OpenAI => (
                format!("{}/v1/chat/completions", config.openai_base_url),
                format!("{}/v1/responses", config.openai_base_url),
                format!("{}/v1/embeddings", config.openai_base_url),
            ),
            BackendKind::Vertex => (
                format!("{}/chat/completions", config.openai_base_url),
                // Vertex does not support Responses API; URL included for completeness
                format!("{}/responses", config.openai_base_url),
                format!("{}/embeddings", config.openai_base_url),
            ),
            BackendKind::Gemini => (
                // openai_base_url already has /openai appended by config,
                // producing .../v1beta/openai/chat/completions
                format!("{}/chat/completions", config.openai_base_url),
                format!("{}/responses", config.openai_base_url),
                // Gemini embeddings: .../v1beta/openai/embeddings
                format!("{}/embeddings", config.openai_base_url),
            ),
            BackendKind::AzureOpenAI => {
                // Azure URL is pre-constructed in config (includes deployment + api-version).
                // Embeddings and Responses URLs are derived by replacing the path component.
                let endpoint = config
                    .openai_base_url
                    .split("/openai/deployments/")
                    .next()
                    .unwrap_or(&config.openai_base_url);
                let api_version = config
                    .openai_base_url
                    .split("api-version=")
                    .nth(1)
                    .unwrap_or("2024-10-21");
                let deployment = config
                    .openai_base_url
                    .split("/openai/deployments/")
                    .nth(1)
                    .and_then(|s| s.split('/').next())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        panic!(
                            "AZURE_OPENAI_API_BASE must contain '/openai/deployments/{{deployment}}', \
                             got: '{}'. Example: https://myresource.openai.azure.com/openai/deployments/gpt-4o",
                            config.openai_base_url
                        )
                    });
                (
                    config.openai_base_url.clone(),
                    // Azure Responses API is not widely available; provide URL for completeness
                    format!("{endpoint}/openai/deployments/{deployment}/responses?api-version={api_version}"),
                    format!("{endpoint}/openai/deployments/{deployment}/embeddings?api-version={api_version}"),
                )
            }
            BackendKind::Anthropic | BackendKind::Bedrock => {
                unreachable!("OpenAIClient should not be constructed for Anthropic/Bedrock backend")
            }
        };

        Self {
            client,
            chat_completions_url,
            responses_url,
            embeddings_url,
            auth: config.backend_auth.clone(),
            backend_kind: config.backend.clone(),
            base_url: config.openai_base_url.clone(),
            provider_id: config.provider_id.clone(),
        }
    }

    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }

    /// Returns the API key/token for use in batch API calls.
    pub fn api_key(&self) -> String {
        match &self.auth {
            BackendAuth::BearerToken(k) => k.clone(),
            BackendAuth::AzureApiKey(k) => k.clone(),
            BackendAuth::GoogleApiKey(k) => k.clone(),
            BackendAuth::AnthropicApiKey(k) => k.clone(),
            BackendAuth::AnthropicAuthToken(k) => k.clone(),
        }
    }

    /// Returns the base URL for batch API calls.
    ///
    /// For Gemini/Vertex the openai_base_url ends in /openai — strip that since the
    /// batch endpoint is not on the OpenAI-compat path.
    pub fn base_url_for_batch(&self) -> String {
        self.base_url
            .trim_end_matches("/openai")
            .trim_end_matches('/')
            .to_string()
    }

    /// Fallback error for unparseable error responses. The backend may return
    /// HTML error pages (e.g., Cloudflare 502) that don't match ErrorResponse.
    fn fallback_error(status: u16) -> openai::errors::ErrorResponse {
        openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message: format!("OpenAI returned status {status}"),
                error_type: "api_error".to_string(),
                param: None,
                code: None,
            },
        }
    }

    async fn send_with_retry(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, OpenAIClientError> {
        super::send_with_retry(
            &self.client,
            &self.chat_completions_url,
            &self.auth,
            req,
            "OpenAI",
        )
        .await
    }

    /// Send a non-streaming chat completion request with retry on 429/5xx.
    ///
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
    pub async fn chat_completion(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(openai::ChatCompletionResponse, u16, RateLimitHeaders), OpenAIClientError> {
        let response = self.send_with_retry(req).await?;
        let status = response.status().as_u16();
        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        let bytes = response.bytes().await.map_err(OpenAIClientError::Request)?;
        let body = parse_chat_completion_response_bytes(&bytes)?;
        Ok((body, status, rate_limits))
    }

    /// Send a streaming chat completion request with retry on 429/5xx.
    /// Returns the raw response and rate limit headers for SSE parsing once a
    /// successful connection is established.
    ///
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
    pub async fn chat_completion_stream(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(reqwest::Response, RateLimitHeaders), OpenAIClientError> {
        let response = self.send_with_retry(req).await?;
        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        Ok((response, rate_limits))
    }

    /// Send a non-streaming Responses API request with retry.
    ///
    /// OpenAI Responses: <https://platform.openai.com/docs/api-reference/responses/create>
    pub async fn responses(
        &self,
        req: &openai::responses::ResponsesRequest,
    ) -> Result<(openai::responses::ResponsesResponse, u16, RateLimitHeaders), OpenAIClientError>
    {
        let response = super::send_with_retry(
            &self.client,
            &self.responses_url,
            &self.auth,
            req,
            "OpenAI Responses",
        )
        .await?;
        let status = response.status().as_u16();
        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        let body = response
            .json::<openai::responses::ResponsesResponse>()
            .await
            .map_err(|e| OpenAIClientError::Deserialization(e.to_string()))?;
        Ok((body, status, rate_limits))
    }

    /// Send a streaming Responses API request with retry.
    /// Returns the raw response for SSE parsing.
    ///
    /// OpenAI Responses streaming: <https://platform.openai.com/docs/api-reference/responses-streaming>
    pub async fn responses_stream(
        &self,
        req: &openai::responses::ResponsesRequest,
    ) -> Result<(reqwest::Response, RateLimitHeaders), OpenAIClientError> {
        let response = super::send_with_retry(
            &self.client,
            &self.responses_url,
            &self.auth,
            req,
            "OpenAI Responses",
        )
        .await?;
        let rate_limits = RateLimitHeaders::from_openai_headers(response.headers());
        Ok((response, rate_limits))
    }

    /// Build a passthrough URL for the given path suffix (e.g., "/v1/audio/speech").
    /// Adjusts for backend-specific URL schemes (Azure deployments, Vertex/Gemini paths).
    pub fn passthrough_url(&self, path: &str) -> String {
        match self.backend_kind {
            BackendKind::OpenAI => format!("{}{}", self.base_url, path),
            BackendKind::AzureOpenAI => {
                // Azure: {endpoint}/openai/deployments/{deployment}/{suffix}?api-version=...
                let endpoint = self
                    .base_url
                    .split("/openai/deployments/")
                    .next()
                    .unwrap_or(&self.base_url);
                let api_version = self
                    .base_url
                    .split("api-version=")
                    .nth(1)
                    .unwrap_or("2024-10-21");
                let deployment = self
                    .base_url
                    .split("/openai/deployments/")
                    .nth(1)
                    .and_then(|s| s.split('/').next())
                    .unwrap_or("");
                // Strip leading /v1/ to get the resource name (e.g., "audio/speech")
                let suffix = path.strip_prefix("/v1/").unwrap_or(path);
                format!(
                    "{endpoint}/openai/deployments/{deployment}/{suffix}?api-version={api_version}"
                )
            }
            BackendKind::Vertex | BackendKind::Gemini => {
                // Vertex/Gemini: base_url already has provider-specific prefix,
                // just append the path without /v1 prefix
                let suffix = path.strip_prefix("/v1/").unwrap_or(path);
                format!("{}/{}", self.base_url, suffix)
            }
            BackendKind::Anthropic | BackendKind::Bedrock => {
                unreachable!("OpenAIClient should not be constructed for Anthropic/Bedrock")
            }
        }
    }

    /// Forward a raw request body to an arbitrary backend endpoint.
    /// No retry: passthrough requests are forwarded once (callers can retry).
    pub async fn raw_passthrough(
        &self,
        url: &str,
        body: bytes::Bytes,
        content_type: &str,
    ) -> Result<(axum::http::StatusCode, axum::http::HeaderMap, bytes::Bytes), OpenAIClientError>
    {
        let mut req = self
            .client
            .post(url)
            .body(body)
            .header("content-type", content_type);
        req = match &self.auth {
            BackendAuth::BearerToken(token) => req.bearer_auth(token),
            BackendAuth::GoogleApiKey(key) => req.header("x-goog-api-key", key),
            BackendAuth::AzureApiKey(key) => req.header("api-key", key),
            BackendAuth::AnthropicApiKey(key) => req.header("x-api-key", key),
            BackendAuth::AnthropicAuthToken(token) => req.bearer_auth(token),
        };

        let response = req.send().await.map_err(OpenAIClientError::Request)?;
        let status = axum::http::StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let mut resp_headers = axum::http::HeaderMap::new();
        if let Some(ct) = response.headers().get("content-type") {
            resp_headers.insert("content-type", ct.clone());
        }
        let resp_body = response.bytes().await.map_err(OpenAIClientError::Request)?;
        Ok((status, resp_headers, resp_body))
    }

    /// Forward an arbitrary HTTP request to the given URL and return the raw response.
    /// Supports any HTTP method (GET, POST, DELETE, PUT, PATCH). No retry.
    /// The caller is responsible for streaming or buffering the response body.
    pub async fn generic_proxy_request(
        &self,
        method: reqwest::Method,
        url: &str,
        content_type: Option<&str>,
        body: Option<bytes::Bytes>,
    ) -> Result<reqwest::Response, OpenAIClientError> {
        let mut builder = self.client.request(method, url);
        if let Some(ct) = content_type {
            builder = builder.header("content-type", ct);
        }
        if let Some(b) = body {
            builder = builder.body(b);
        }
        builder = match &self.auth {
            BackendAuth::BearerToken(token) => builder.bearer_auth(token),
            BackendAuth::GoogleApiKey(key) => builder.header("x-goog-api-key", key),
            BackendAuth::AzureApiKey(key) => builder.header("api-key", key),
            BackendAuth::AnthropicApiKey(key) => builder.header("x-api-key", key),
            BackendAuth::AnthropicAuthToken(token) => builder.bearer_auth(token),
        };
        builder.send().await.map_err(OpenAIClientError::Request)
    }

    /// Forward a raw embeddings request body to the backend embeddings endpoint.
    /// No retry: embeddings are idempotent but we keep it simple, callers can retry.
    ///
    /// OpenAI: <https://platform.openai.com/docs/api-reference/embeddings/create>
    pub async fn embeddings_passthrough(
        &self,
        body: bytes::Bytes,
        content_type: &str,
    ) -> Result<(axum::http::StatusCode, axum::http::HeaderMap, bytes::Bytes), OpenAIClientError>
    {
        self.raw_passthrough(&self.embeddings_url, body, content_type)
            .await
    }
}

fn parse_chat_completion_response_bytes(
    bytes: &[u8],
) -> Result<openai::ChatCompletionResponse, OpenAIClientError> {
    let mut value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(parse_err) => {
            return if let Some(err) = error_in_success_body(bytes) {
                Err(err)
            } else {
                Err(OpenAIClientError::Deserialization(parse_err.to_string()))
            };
        }
    };

    normalize_chat_completion_response_value(&mut value);

    match serde_json::from_value::<openai::ChatCompletionResponse>(value) {
        Ok(body) => match error_in_finished_choices(&body) {
            // A well-formed 200 body can still carry a per-choice
            // finish_reason "error" with no top-level error envelope (some
            // OpenAI-compatible gateways signal a mid-generation failure this
            // way). Surface it instead of returning a truncated, apparently
            // successful completion (the streaming path does the same).
            Some(err) => Err(err),
            None => Ok(body),
        },
        Err(parse_err) => {
            // OpenAI-compatible gateways (e.g. OpenRouter) can return an error
            // inside a 2xx body when the upstream model fails mid-request. A
            // valid completion requires `choices`, so an error envelope always
            // lands here. Surface it as an ApiError instead of a confusing
            // deserialization failure; otherwise report the parse error.
            // <https://openrouter.ai/docs/api/reference/errors-and-debugging>
            if let Some(err) = error_in_success_body(bytes) {
                Err(err)
            } else {
                Err(OpenAIClientError::Deserialization(parse_err.to_string()))
            }
        }
    }
}

/// Detect a per-choice `finish_reason: "error"` in an otherwise well-formed 200
/// completion.
///
/// Some OpenAI-compatible gateways signal a mid-generation failure with a valid
/// response shape whose choice carries `finish_reason: "error"` (and no top-level
/// `error` envelope, so [`error_in_success_body`] never fires). Without this the
/// response would map to a normal `stop`/`end_turn` and the failure would be
/// silently swallowed. Returns a 502 `ApiError` so callers surface it like any
/// other upstream failure.
fn error_in_finished_choices(body: &openai::ChatCompletionResponse) -> Option<OpenAIClientError> {
    let has_error = body
        .choices
        .iter()
        .any(|c| c.finish_reason == Some(openai::FinishReason::Error));
    if !has_error {
        return None;
    }
    Some(OpenAIClientError::ApiError {
        status: 502,
        error: openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message: "upstream returned finish_reason \"error\"".to_string(),
                error_type: "api_error".to_string(),
                param: None,
                code: None,
            },
        },
    })
}

/// Detect an error returned inside a 2xx response body.
///
/// Some OpenAI-compatible gateways (notably OpenRouter) return errors in a 200
/// response — a top-level `error` object — rather than via the HTTP status when
/// the upstream model fails after the request was accepted. OpenRouter puts an
/// HTTP-like status in `error.code` (a number); OpenAI-shaped envelopes use a
/// string `code`. Returns `None` for a normal completion (no `error` key) or a
/// body that is not JSON.
fn error_in_success_body(bytes: &[u8]) -> Option<OpenAIClientError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let err = value.get("error").filter(|e| !e.is_null())?;
    let code_field = err.get("code");
    let numeric_code = code_field.and_then(serde_json::Value::as_u64);
    // OpenRouter encodes the upstream HTTP status in error.code as a number; use
    // it only when it is a plausible HTTP status (400..=599). A numeric code
    // outside that range is a provider-specific code, not a status, so fall back
    // to 502 (the upstream model failed inside an otherwise-200 reply) and keep
    // the code below rather than discarding it.
    let in_range = |c: &u64| (400..=599).contains(c);
    let status = numeric_code
        .filter(in_range)
        .map(|c| c as u16)
        .unwrap_or(502);
    let message = err
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("upstream returned an error in a 2xx response")
        .to_string();
    let error_type = err
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("api_error")
        .to_string();
    // Preserve the original code: a string code as-is, or a numeric code that was
    // NOT consumed as the HTTP status (out of range), so it is not lost downstream.
    let code = code_field
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .or_else(|| numeric_code.filter(|c| !in_range(c)).map(|c| c.to_string()));
    Some(OpenAIClientError::ApiError {
        status,
        error: openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message,
                error_type,
                param: None,
                code,
            },
        },
    })
}

/// Errors from the OpenAI HTTP client.
#[derive(Debug)]
pub enum OpenAIClientError {
    /// Transport-level failure (DNS, TLS, connection refused, timeout).
    Request(reqwest::Error),
    /// Backend returned 2xx but the body was not valid ChatCompletionResponse JSON
    /// (and was not a recognizable error envelope). Carries the parse error text.
    Deserialization(String),
    /// Backend returned a non-2xx status with a parseable OpenAI error body.
    ApiError {
        status: u16,
        error: openai::errors::ErrorResponse,
    },
}

impl std::fmt::Display for OpenAIClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "request failed: {e}"),
            Self::Deserialization(e) => write!(f, "response deserialization failed: {e}"),
            Self::ApiError { status, error } => {
                write!(f, "OpenAI API error ({status}): {}", error.error.message)
            }
        }
    }
}

impl OpenAIClientError {
    /// HTTP status code from an API error, or 500 for transport/deserialization errors.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::ApiError { status, .. } => *status,
            _ => 500,
        }
    }
}

impl RetryableError for OpenAIClientError {
    fn from_request(e: reqwest::Error) -> Self {
        Self::Request(e)
    }

    fn from_api_response(status: u16, body: &str) -> Self {
        let error =
            serde_json::from_str::<openai::errors::ErrorResponse>(body).unwrap_or_else(|e| {
                tracing::debug!("failed to parse OpenAI error response: {e}");
                OpenAIClient::fallback_error(status)
            });
        Self::ApiError { status, error }
    }
}

#[cfg(test)]
mod tests;
