// reqwest client for calling OpenAI endpoints
// PLAN.md lines 649-650

use crate::config::Config;
use anthropic_openai_translate::openai;
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 500;

#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    chat_completions_url: String,
    api_key: String,
}

impl OpenAIClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            chat_completions_url: format!("{}/v1/chat/completions", config.openai_base_url),
            api_key: config.openai_api_key.clone(),
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

    /// Send a non-streaming chat completion request with retry on 429/5xx.
    pub async fn chat_completion(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(openai::ChatCompletionResponse, u16), OpenAIClientError> {
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .client
                .post(&self.chat_completions_url)
                .bearer_auth(&self.api_key)
                .json(req)
                .send()
                .await
                .map_err(OpenAIClientError::Request)?;

            let status = response.status().as_u16();

            if !response.status().is_success() {
                // On retryable status, retry unless we've exhausted attempts
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

            let body = response
                .json::<openai::ChatCompletionResponse>()
                .await
                .map_err(OpenAIClientError::Deserialization)?;

            return Ok((body, status));
        }

        // Unreachable: the loop always returns on success or final failure.
        unreachable!("retry loop should always return")
    }

    /// Send a streaming chat completion request and return the raw response for SSE parsing.
    pub async fn chat_completion_stream(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, OpenAIClientError> {
        let response = self
            .client
            .post(&self.chat_completions_url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await
            .map_err(OpenAIClientError::Request)?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let error_body = response
                .json::<openai::errors::ErrorResponse>()
                .await
                .unwrap_or_else(|_| Self::fallback_error(status));
            return Err(OpenAIClientError::ApiError {
                status,
                error: error_body,
            });
        }

        Ok(response)
    }
}

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
}
