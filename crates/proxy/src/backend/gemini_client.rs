// reqwest client for calling native Gemini API endpoints
// Developer API: https://ai.google.dev/api/generate-content

use super::{build_http_client, RateLimitHeaders, RetryableError};
use crate::config::{BackendAuth, Config};
use anthropic_openai_translate::gemini;
use reqwest::Client;

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

    async fn send_with_retry(
        &self,
        req: &gemini::GenerateContentRequest,
        url: &str,
    ) -> Result<reqwest::Response, GeminiClientError> {
        super::send_with_retry(&self.client, url, &self.auth, req, "Gemini").await
    }

    /// Send a non-streaming generateContent request with retry on 429/5xx.
    ///
    /// See <https://ai.google.dev/api/generate-content#v1beta.models.generateContent>
    pub async fn generate_content(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<(gemini::GenerateContentResponse, u16, RateLimitHeaders), GeminiClientError> {
        let url = self.generate_content_url(model);
        let response = self.send_with_retry(req, &url).await?;
        let status = response.status().as_u16();
        let body = response
            .json::<gemini::GenerateContentResponse>()
            .await
            .map_err(GeminiClientError::Deserialization)?;
        // Gemini does not send rate limit headers; return defaults.
        Ok((body, status, RateLimitHeaders::default()))
    }

    /// Send a streaming generateContent request with retry on 429/5xx.
    /// Returns the raw response and (default) rate limit headers once a
    /// successful connection is established.
    ///
    /// See <https://ai.google.dev/api/generate-content#v1beta.models.streamGenerateContent>
    pub async fn generate_content_stream(
        &self,
        req: &gemini::GenerateContentRequest,
        model: &str,
    ) -> Result<(reqwest::Response, RateLimitHeaders), GeminiClientError> {
        let url = self.stream_generate_content_url(model);
        let response = self.send_with_retry(req, &url).await?;
        // Gemini does not send rate limit headers; return defaults.
        Ok((response, RateLimitHeaders::default()))
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

impl RetryableError for GeminiClientError {
    fn from_request(e: reqwest::Error) -> Self {
        Self::Request(e)
    }

    fn from_api_response(status: u16, body: &str) -> Self {
        let error =
            serde_json::from_str::<gemini::errors::ErrorResponse>(body).unwrap_or_else(|e| {
                tracing::debug!("failed to parse Gemini error response: {e}");
                GeminiClient::fallback_error(status)
            });
        Self::ApiError { status, error }
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
            log_bodies: false,
            openai_api_format: crate::config::OpenAIApiFormat::Chat,
        };
        // Should not panic
        let _client = GeminiClient::new(&config);
    }
}
