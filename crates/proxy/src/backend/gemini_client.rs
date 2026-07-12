// Gemini native HTTP client for generateContent / streamGenerateContent endpoints.
// No OpenAI translation: sends and receives Gemini-native JSON directly.

use super::{build_http_client, RetryableError};
use crate::config::{BackendAuth, TlsConfig};
use anyllm_translate::gemini::{GenerateContentRequest, GenerateContentResponse};
use reqwest::Client;

/// HTTP client for Google Gemini's native generateContent API.
#[derive(Clone)]
pub struct GeminiNativeClient {
    client: Client,
    base_url: String,
    // Built once from the API key so per-request sends borrow it instead of
    // cloning the key on every call.
    auth: BackendAuth,
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

impl RetryableError for GeminiClientError {
    fn from_request(e: reqwest::Error) -> Self {
        Self::Transport(e.to_string())
    }

    fn from_api_response(status: u16, body: &str) -> Self {
        Self::ApiError {
            status,
            body: body.to_string(),
        }
    }
}

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
        let client = build_http_client(tls, false);
        Self {
            client,
            base_url,
            auth: BackendAuth::GoogleApiKey(api_key),
            big_model,
            small_model,
        }
    }

    /// The model ID used for sonnet/opus-class requests.
    pub fn big_model(&self) -> &str {
        &self.big_model
    }

    /// The model ID used for haiku-class requests.
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
        // Shared retry helper: retries 429/5xx with backoff and honors Retry-After,
        // matching every other backend. GoogleApiKey auth sets the x-goog-api-key
        // header; .json(body) sets Content-Type. Non-2xx becomes ApiError.
        let resp = super::send_with_retry::<GeminiClientError>(
            &self.client,
            &url,
            &self.auth,
            body,
            "Gemini",
        )
        .await?;

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
        // Retry on 429/5xx before SSE starts (same policy as the non-streaming
        // path). Once a 2xx response is returned, streaming proceeds as before.
        super::send_with_retry::<GeminiClientError>(
            &self.client,
            &url,
            &self.auth,
            body,
            "Gemini stream",
        )
        .await
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

    #[test]
    fn retryable_error_from_api_response_preserves_status_and_body() {
        // The shared retry helper turns a non-2xx upstream response into this
        // variant; status and body must round-trip so backoff/classification work.
        let e = GeminiClientError::from_api_response(429, "{\"error\":\"quota\"}");
        match e {
            GeminiClientError::ApiError { status, body } => {
                assert_eq!(status, 429);
                assert_eq!(body, "{\"error\":\"quota\"}");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[test]
    fn api_error_classifies_via_backend_error() {
        // Through the unified BackendError, a Gemini 429/403/5xx must classify
        // and surface its status the same way other backends do.
        use crate::backend::BackendError;
        let cases: &[(u16, &str)] = &[
            (401, "client_error"),
            (403, "client_error"),
            (404, "client_error"),
            (429, "rate_limit"),
            (500, "backend_error"),
            (503, "backend_error"),
            (504, "timeout"),
        ];
        for (status, kind) in cases {
            let be = BackendError::from(GeminiClientError::ApiError {
                status: *status,
                body: "upstream error".to_string(),
            });
            assert_eq!(be.status_code(), *status, "status {status}");
            assert_eq!(be.error_kind(), *kind, "kind for status {status}");
            let (msg, s) = be.api_error_details().expect("api error details");
            assert_eq!(s, *status);
            assert_eq!(msg, "upstream error");
        }
    }
}
