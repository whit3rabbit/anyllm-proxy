// Gemini native HTTP client for generateContent / streamGenerateContent endpoints.
// No OpenAI translation: sends and receives Gemini-native JSON directly.

use super::build_http_client;
use crate::config::TlsConfig;
use anyllm_translate::gemini::{GenerateContentRequest, GenerateContentResponse};
use reqwest::Client;

/// HTTP client for Google Gemini's native generateContent API.
#[derive(Clone)]
pub struct GeminiNativeClient {
    client: Client,
    base_url: String,
    api_key: String,
    big_model: String,
    small_model: String,
}

/// Error type for the Gemini native client.
#[derive(Debug)]
pub enum GeminiClientError {
    /// Transport-level error (connection, timeout, DNS).
    Transport(String),
    /// Upstream returned a non-success status.
    ApiError { status: u16, body: String },
    /// Response body could not be deserialized.
    Deserialize(String),
}

impl std::fmt::Display for GeminiClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "Gemini transport error: {e}"),
            Self::ApiError { status, body } => {
                write!(f, "Gemini API error (status {status}): {body}")
            }
            Self::Deserialize(e) => write!(f, "Gemini deserialization error: {e}"),
        }
    }
}

impl std::error::Error for GeminiClientError {}

impl GeminiNativeClient {
    /// Create a new Gemini native client.
    ///
    /// `base_url` should be the Gemini API root, e.g.
    /// `https://generativelanguage.googleapis.com/v1beta`.
    pub fn new(
        base_url: String,
        api_key: String,
        big_model: String,
        small_model: String,
        tls: &TlsConfig,
    ) -> Self {
        let client = build_http_client(tls);
        Self {
            client,
            base_url,
            api_key,
            big_model,
            small_model,
        }
    }

    pub fn big_model(&self) -> &str {
        &self.big_model
    }

    pub fn small_model(&self) -> &str {
        &self.small_model
    }

    /// Map an Anthropic model name to the configured Gemini model.
    pub fn map_model(&self, anthropic_model: &str) -> String {
        let lower = anthropic_model.to_lowercase();
        if lower.contains("haiku") {
            self.small_model.clone()
        } else {
            self.big_model.clone()
        }
    }

    /// Build the generateContent URL for a given model.
    fn generate_url(&self, model: &str) -> String {
        format!(
            "{}/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            model
        )
    }

    /// Build the streamGenerateContent URL for a given model.
    fn stream_url(&self, model: &str) -> String {
        format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url.trim_end_matches('/'),
            model
        )
    }

    /// Non-streaming: POST generateContent, parse response.
    pub async fn generate_content(
        &self,
        body: &GenerateContentRequest,
        model: &str,
    ) -> Result<GenerateContentResponse, GeminiClientError> {
        let url = self.generate_url(model);
        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| GeminiClientError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(GeminiClientError::ApiError {
                status: status.as_u16(),
                body: body_text,
            });
        }

        resp.json::<GenerateContentResponse>()
            .await
            .map_err(|e| GeminiClientError::Deserialize(e.to_string()))
    }

    /// Streaming: POST streamGenerateContent, return raw Response for SSE reading.
    pub async fn generate_content_stream(
        &self,
        body: &GenerateContentRequest,
        model: &str,
    ) -> Result<reqwest::Response, GeminiClientError> {
        let url = self.stream_url(model);
        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| GeminiClientError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(GeminiClientError::ApiError {
                status: status.as_u16(),
                body: body_text,
            });
        }

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client(base_url: &str) -> GeminiNativeClient {
        GeminiNativeClient::new(
            base_url.to_string(),
            "test-key".to_string(),
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
            &TlsConfig::default(),
        )
    }

    #[test]
    fn generate_url_construction() {
        let c = test_client("https://generativelanguage.googleapis.com/v1beta");
        assert_eq!(
            c.generate_url("gemini-2.5-pro"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn stream_url_construction() {
        let c = test_client("https://generativelanguage.googleapis.com/v1beta");
        assert_eq!(
            c.stream_url("gemini-2.5-pro"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn map_model_haiku_to_small() {
        let c = test_client("https://example.com");
        assert_eq!(c.map_model("claude-3-haiku-20240307"), "gemini-2.5-flash");
        assert_eq!(c.map_model("claude-sonnet-4-6"), "gemini-2.5-pro");
    }

    #[test]
    fn map_model_case_insensitive() {
        let c = test_client("https://example.com");
        assert_eq!(c.map_model("Claude-3-HAIKU-20240307"), "gemini-2.5-flash");
    }

    #[test]
    fn base_url_trailing_slash_stripped() {
        let c = test_client("https://example.com/v1beta/");
        let url = c.generate_url("pro");
        assert!(
            url.contains("/v1beta/models/pro:generateContent"),
            "got: {url}"
        );
        assert!(!url.contains("//models"), "double slash in: {url}");
    }

    #[test]
    fn stream_url_trailing_slash_stripped() {
        let c = test_client("https://example.com/v1beta/");
        let url = c.stream_url("pro");
        assert!(!url.contains("//models"), "double slash in: {url}");
    }

    #[test]
    fn error_display_transport() {
        let e = GeminiClientError::Transport("connection refused".to_string());
        let s = e.to_string();
        assert!(s.contains("transport"), "got: {s}");
        assert!(s.contains("connection refused"), "got: {s}");
    }

    #[test]
    fn error_display_api() {
        let e = GeminiClientError::ApiError {
            status: 429,
            body: "rate limited".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("429"), "got: {s}");
        assert!(s.contains("rate limited"), "got: {s}");
    }

    #[test]
    fn error_display_deserialize() {
        let e = GeminiClientError::Deserialize("unexpected token".to_string());
        let s = e.to_string();
        assert!(s.contains("deserialization"), "got: {s}");
    }

    #[test]
    fn model_accessors() {
        let c = test_client("https://example.com");
        assert_eq!(c.big_model(), "gemini-2.5-pro");
        assert_eq!(c.small_model(), "gemini-2.5-flash");
    }
}
