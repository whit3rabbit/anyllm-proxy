// reqwest client for calling OpenAI endpoints
// PLAN.md lines 649-650

use crate::config::{BackendAuth, BackendKind, Config};
use anthropic_openai_translate::openai;
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 500;

/// HTTP client for OpenAI-compatible Chat Completions APIs with retry logic.
/// Works with both OpenAI and Vertex AI OpenAI-compatible endpoints.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    chat_completions_url: String,
    auth: BackendAuth,
}

impl OpenAIClient {
    /// Create a new client from proxy configuration.
    /// Configures mTLS identity and custom CA cert if present in config.
    pub fn new(config: &Config) -> Self {
        let mut builder = Client::builder();

        if let Some((ref p12_bytes, ref password)) = config.tls.p12_identity {
            let identity = reqwest::Identity::from_pkcs12_der(p12_bytes, password)
                .expect("P12 identity was validated at startup");
            builder = builder.identity(identity);
        }

        if let Some(ref ca_pem) = config.tls.ca_cert_pem {
            let cert =
                reqwest::Certificate::from_pem(ca_pem).expect("CA cert was validated at startup");
            builder = builder.add_root_certificate(cert);
        }

        let client = builder.build().expect("failed to build HTTP client");

        // OpenAI base URL does not include /v1, Vertex base URL ends at /openapi
        let chat_completions_url = match config.backend {
            BackendKind::OpenAI => {
                format!("{}/v1/chat/completions", config.openai_base_url)
            }
            BackendKind::Vertex => {
                format!("{}/chat/completions", config.openai_base_url)
            }
        };

        Self {
            client,
            chat_completions_url,
            auth: config.backend_auth.clone(),
        }
    }

    /// Check if a status code is retryable (429 or 5xx).
    fn is_retryable(status: u16) -> bool {
        status == 429 || (500..=599).contains(&status)
    }

    /// Parse retry-after header value in seconds.
    fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }

    /// Build a fallback error response when the body cannot be parsed.
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

    /// Compute backoff delay with jitter.
    fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(ra) = retry_after {
            return ra;
        }
        let base = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt));
        // Add up to 25% jitter (deterministic upper bound; true randomness not needed here)
        let jitter_ms = (base.as_millis() as u64) / 4;
        base + Duration::from_millis(jitter_ms)
    }

    /// Send a request with retry on 429/5xx. Returns the raw successful response.
    async fn send_with_retry(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, OpenAIClientError> {
        for attempt in 0..=MAX_RETRIES {
            let mut rb = self.client.post(&self.chat_completions_url).json(req);
            rb = match &self.auth {
                BackendAuth::BearerToken(token) => rb.bearer_auth(token),
                BackendAuth::GoogleApiKey(key) => rb.header("x-goog-api-key", key),
            };
            let response = rb.send().await.map_err(OpenAIClientError::Request)?;

            let status = response.status().as_u16();

            if !response.status().is_success() {
                if Self::is_retryable(status) && attempt < MAX_RETRIES {
                    let retry_after = Self::parse_retry_after(response.headers());
                    let delay = Self::backoff_delay(attempt, retry_after);
                    tracing::warn!(
                        status,
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        delay_ms = delay.as_millis() as u64,
                        "retryable error from OpenAI, backing off"
                    );
                    sleep(delay).await;
                    continue;
                }

                let error_body = response
                    .json::<openai::errors::ErrorResponse>()
                    .await
                    .unwrap_or_else(|_| Self::fallback_error(status));
                return Err(OpenAIClientError::ApiError {
                    status,
                    error: error_body,
                });
            }

            return Ok(response);
        }

        unreachable!("retry loop should always return")
    }

    /// Send a non-streaming chat completion request with retry on 429/5xx.
    ///
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
    pub async fn chat_completion(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(openai::ChatCompletionResponse, u16), OpenAIClientError> {
        let response = self.send_with_retry(req).await?;
        let status = response.status().as_u16();
        let body = response
            .json::<openai::ChatCompletionResponse>()
            .await
            .map_err(OpenAIClientError::Deserialization)?;
        Ok((body, status))
    }

    /// Send a streaming chat completion request with retry on 429/5xx.
    /// Returns the raw response for SSE parsing once a successful connection is established.
    ///
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
    pub async fn chat_completion_stream(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, OpenAIClientError> {
        self.send_with_retry(req).await
    }
}

/// Errors from the OpenAI HTTP client.
#[derive(Debug)]
pub enum OpenAIClientError {
    Request(reqwest::Error),
    Deserialization(reqwest::Error),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_retryable_429() {
        assert!(OpenAIClient::is_retryable(429));
    }

    #[test]
    fn is_retryable_500() {
        assert!(OpenAIClient::is_retryable(500));
        assert!(OpenAIClient::is_retryable(502));
        assert!(OpenAIClient::is_retryable(503));
        assert!(OpenAIClient::is_retryable(599));
    }

    #[test]
    fn is_not_retryable_400() {
        assert!(!OpenAIClient::is_retryable(400));
        assert!(!OpenAIClient::is_retryable(401));
        assert!(!OpenAIClient::is_retryable(404));
    }

    #[test]
    fn backoff_respects_retry_after() {
        let delay = OpenAIClient::backoff_delay(0, Some(Duration::from_secs(5)));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn backoff_increases_with_attempt() {
        let d0 = OpenAIClient::backoff_delay(0, None);
        let d1 = OpenAIClient::backoff_delay(1, None);
        let d2 = OpenAIClient::backoff_delay(2, None);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn parse_retry_after_valid() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "3".parse().unwrap());
        let dur = OpenAIClient::parse_retry_after(&headers);
        assert_eq!(dur, Some(Duration::from_secs(3)));
    }

    #[test]
    fn parse_retry_after_missing() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(OpenAIClient::parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_non_numeric() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        // Date format is not parsed; should return None
        assert_eq!(OpenAIClient::parse_retry_after(&headers), None);
    }

    #[test]
    fn client_builds_without_tls() {
        use crate::config::{BackendKind, ModelMapping, TlsConfig};
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
        };
        // Should not panic
        let _client = OpenAIClient::new(&config);
    }

    #[test]
    fn client_builds_vertex_config() {
        use crate::config::{BackendKind, ModelMapping, TlsConfig};
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
        };
        let client = OpenAIClient::new(&config);
        // Verify URL construction for Vertex (no /v1 prefix)
        assert!(client
            .chat_completions_url
            .ends_with("/openapi/chat/completions"));
    }
}
