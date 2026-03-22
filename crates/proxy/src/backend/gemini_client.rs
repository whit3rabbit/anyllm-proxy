// reqwest client for calling native Gemini API endpoints
// Developer API: https://ai.google.dev/api/generate-content

use super::build_http_client;
use crate::config::{BackendAuth, Config};
use anthropic_openai_translate::gemini;
use reqwest::Client;
use tokio::time::sleep;

/// HTTP client for Gemini's native generateContent API with retry logic.
///
/// See <https://ai.google.dev/api/generate-content>
#[derive(Clone)]
pub struct GeminiClient {
    client: Client,
    base_url: String,
    auth: BackendAuth,
}

impl GeminiClient {
    /// Create a new client from proxy configuration.
    pub fn new(config: &Config) -> Self {
        let client = build_http_client(&config.tls);

        Self {
            client,
            base_url: config.openai_base_url.clone(),
            auth: config.backend_auth.clone(),
        }
    }

    /// Construct the generateContent URL for a given model.
    fn generate_content_url(&self, model: &str) -> String {
        format!("{}/models/{}:generateContent", self.base_url, model)
    }

    /// Construct the streamGenerateContent URL for a given model.
    fn stream_generate_content_url(&self, model: &str) -> String {
        format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url, model
        )
    }

    /// Build a fallback error response when the body cannot be parsed.
    fn fallback_error(status: u16) -> gemini::errors::ErrorResponse {
        gemini::errors::ErrorResponse {
            error: gemini::errors::ErrorDetail {
                code: status,
                message: format!("Gemini returned status {status}"),
                status: "UNKNOWN".to_string(),
            },
        }
    }

    /// Send a request with retry on 429/5xx. Returns the raw successful response.
    async fn send_with_retry(
        &self,
        req: &gemini::GenerateContentRequest,
        url: &str,
    ) -> Result<reqwest::Response, GeminiClientError> {
        for attempt in 0..=crate::backend::MAX_RETRIES {
            let mut rb = self.client.post(url).json(req);
            rb = match &self.auth {
                BackendAuth::BearerToken(token) => rb.bearer_auth(token),
                BackendAuth::GoogleApiKey(key) => rb.header("x-goog-api-key", key),
            };
            let response = rb.send().await.map_err(GeminiClientError::Request)?;

            let status = response.status().as_u16();

            if !response.status().is_success() {
                if crate::backend::is_retryable(status) && attempt < crate::backend::MAX_RETRIES {
                    let retry_after = crate::backend::parse_retry_after(response.headers());
                    let delay = crate::backend::backoff_delay(attempt, retry_after);
                    tracing::warn!(
                        status,
                        attempt = attempt + 1,
                        max_retries = crate::backend::MAX_RETRIES,
                        delay_ms = delay.as_millis() as u64,
                        "retryable error from Gemini, backing off"
                    );
                    sleep(delay).await;
                    continue;
                }

                let error_body = response
                    .json::<gemini::errors::ErrorResponse>()
                    .await
                    .unwrap_or_else(|_| Self::fallback_error(status));
                return Err(GeminiClientError::ApiError {
                    status,
                    error: error_body,
                });
            }

            return Ok(response);
        }

        unreachable!("retry loop should always return")
    }

    /// Send a non-streaming generateContent request with retry on 429/5xx.
    ///
    /// See <https://ai.google.dev/api/generate-content#v1beta.models.generateContent>
    pub async fn generate_content(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<(gemini::GenerateContentResponse, u16), GeminiClientError> {
        let url = self.generate_content_url(model);
        let response = self.send_with_retry(req, &url).await?;
        let status = response.status().as_u16();
        let body = response
            .json::<gemini::GenerateContentResponse>()
            .await
            .map_err(GeminiClientError::Deserialization)?;
        Ok((body, status))
    }

    /// Send a streaming generateContent request with retry on 429/5xx.
    /// Returns the raw response for SSE parsing once a successful connection is established.
    ///
    /// See <https://ai.google.dev/api/generate-content#v1beta.models.streamGenerateContent>
    pub async fn generate_content_stream(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<reqwest::Response, GeminiClientError> {
        let url = self.stream_generate_content_url(model);
        self.send_with_retry(req, &url).await
    }
}

/// Errors from the Gemini HTTP client.
#[derive(Debug)]
pub enum GeminiClientError {
    Request(reqwest::Error),
    Deserialization(reqwest::Error),
    ApiError {
        status: u16,
        error: gemini::errors::ErrorResponse,
    },
}

impl std::fmt::Display for GeminiClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "request failed: {e}"),
            Self::Deserialization(e) => write!(f, "response deserialization failed: {e}"),
            Self::ApiError { status, error } => {
                write!(f, "Gemini API error ({status}): {}", error.error.message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_content_url_construction() {
        let client = GeminiClient {
            client: Client::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            auth: BackendAuth::GoogleApiKey("test".into()),
        };
        assert_eq!(
            client.generate_content_url("gemini-2.5-pro"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn stream_generate_content_url_construction() {
        let client = GeminiClient {
            client: Client::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            auth: BackendAuth::GoogleApiKey("test".into()),
        };
        assert_eq!(
            client.stream_generate_content_url("gemini-2.5-flash"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn client_builds_gemini_config() {
        use crate::config::{BackendKind, ModelMapping, TlsConfig};
        let config = Config {
            backend: BackendKind::Gemini,
            openai_api_key: String::new(),
            openai_base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            listen_port: 3000,
            model_mapping: ModelMapping {
                big_model: "gemini-2.5-pro".into(),
                small_model: "gemini-2.5-flash".into(),
            },
            tls: TlsConfig::default(),
            backend_auth: BackendAuth::GoogleApiKey("test-key".into()),
        };
        // Should not panic
        let _client = GeminiClient::new(&config);
    }
}
