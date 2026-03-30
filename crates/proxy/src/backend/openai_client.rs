// reqwest client for calling OpenAI endpoints

use super::{build_http_client, RateLimitHeaders, RetryableError};
use crate::config::{BackendAuth, BackendKind, Config};
use anyllm_translate::openai;
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
                    .unwrap_or("");
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
        }
    }

    /// Returns the API key/token for use in batch API calls.
    pub fn api_key(&self) -> String {
        match &self.auth {
            BackendAuth::BearerToken(k) => k.clone(),
            BackendAuth::AzureApiKey(k) => k.clone(),
            BackendAuth::GoogleApiKey(k) => k.clone(),
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
        let body = response
            .json::<openai::ChatCompletionResponse>()
            .await
            .map_err(OpenAIClientError::Deserialization)?;
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
            .map_err(OpenAIClientError::Deserialization)?;
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

/// Errors from the OpenAI HTTP client.
#[derive(Debug)]
pub enum OpenAIClientError {
    /// Transport-level failure (DNS, TLS, connection refused, timeout).
    Request(reqwest::Error),
    /// Backend returned 2xx but the body was not valid ChatCompletionResponse JSON.
    Deserialization(reqwest::Error),
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
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn is_retryable_429() {
        assert!(crate::backend::is_retryable(429));
    }

    #[test]
    fn is_retryable_500() {
        assert!(crate::backend::is_retryable(500));
        assert!(crate::backend::is_retryable(502));
        assert!(crate::backend::is_retryable(503));
        assert!(crate::backend::is_retryable(599));
    }

    #[test]
    fn is_retryable_408() {
        assert!(crate::backend::is_retryable(408));
    }

    #[test]
    fn is_not_retryable_400() {
        assert!(!crate::backend::is_retryable(400));
        assert!(!crate::backend::is_retryable(401));
        assert!(!crate::backend::is_retryable(404));
        assert!(!crate::backend::is_retryable(409));
    }

    #[test]
    fn backoff_respects_retry_after() {
        let delay = crate::backend::backoff_delay(0, Some(Duration::from_secs(5)));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn backoff_increases_with_attempt() {
        let d0 = crate::backend::backoff_delay(0, None);
        let d1 = crate::backend::backoff_delay(1, None);
        let d2 = crate::backend::backoff_delay(2, None);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn parse_retry_after_valid() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "3".parse().unwrap());
        let dur = crate::backend::parse_retry_after(&headers);
        assert_eq!(dur, Some(Duration::from_secs(3)));
    }

    #[test]
    fn parse_retry_after_missing() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(crate::backend::parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_http_date_future() {
        // Use a date far in the future so it's always ahead of now
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Wed, 21 Oct 2037 07:28:00 GMT".parse().unwrap(),
        );
        let dur = crate::backend::parse_retry_after(&headers);
        assert!(dur.is_some(), "future HTTP date should parse to Some");
        assert!(dur.unwrap().as_secs() > 0);
    }

    #[test]
    fn parse_retry_after_http_date_past() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Mon, 01 Jan 2024 00:00:00 GMT".parse().unwrap(),
        );
        // Past date: no wait needed
        assert_eq!(crate::backend::parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_garbage() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "not-a-date-or-number".parse().unwrap());
        assert_eq!(crate::backend::parse_retry_after(&headers), None);
    }

    #[test]
    fn client_builds_without_tls() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::OpenAI,
            openai_api_key: "test".into(),
            openai_base_url: "https://api.openai.com".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gpt-4o".into(),
                small_model: "gpt-4o-mini".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::BearerToken("test".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        // Should not panic
        let _client = OpenAIClient::new(&config);
    }

    #[test]
    fn client_builds_vertex_config() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::Vertex,
            openai_api_key: String::new(),
            openai_base_url: "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gemini-2.5-pro".into(),
                small_model: "gemini-2.5-flash".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::GoogleApiKey("test-key".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        let client = OpenAIClient::new(&config);
        // Verify URL construction for Vertex (no /v1 prefix)
        assert!(client
            .chat_completions_url
            .ends_with("/openapi/chat/completions"));
    }

    #[test]
    fn embeddings_url_openai() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::OpenAI,
            openai_api_key: "test".into(),
            openai_base_url: "https://api.openai.com".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gpt-4o".into(),
                small_model: "gpt-4o-mini".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::BearerToken("test".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        let client = OpenAIClient::new(&config);
        assert_eq!(
            client.embeddings_url,
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn embeddings_url_vertex() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::Vertex,
            openai_api_key: String::new(),
            openai_base_url: "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gemini-2.5-pro".into(),
                small_model: "gemini-2.5-flash".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::GoogleApiKey("test-key".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        let client = OpenAIClient::new(&config);
        assert!(
            client.embeddings_url.ends_with("/openapi/embeddings"),
            "got: {}",
            client.embeddings_url
        );
    }

    #[test]
    fn embeddings_url_gemini() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::Gemini,
            openai_api_key: String::new(),
            // Config appends /openai to the base, so this is what arrives here
            openai_base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gemini-2.5-pro".into(),
                small_model: "gemini-2.5-flash".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::GoogleApiKey("test-gemini-key".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        let client = OpenAIClient::new(&config);
        assert_eq!(
            client.embeddings_url,
            "https://generativelanguage.googleapis.com/v1beta/openai/embeddings"
        );
    }

    #[test]
    fn azure_url_passthrough() {
        use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
        let config = Config {
            backend: BackendKind::AzureOpenAI,
            openai_api_key: String::new(),
            openai_base_url: "https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gpt-4o".into(),
                small_model: "gpt-4o-mini".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::AzureApiKey("test-azure-key".into()),
            log_bodies: false,
            expose_degradation_warnings: false,
            openai_api_format: OpenAIApiFormat::Chat,
        };
        let client = OpenAIClient::new(&config);
        // Chat completions URL is the pre-built URL, unchanged
        assert_eq!(
            client.chat_completions_url,
            "https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21"
        );
        // Embeddings URL is derived from the endpoint and deployment
        assert_eq!(
            client.embeddings_url,
            "https://myresource.openai.azure.com/openai/deployments/gpt-4o/embeddings?api-version=2024-10-21"
        );
    }
}
