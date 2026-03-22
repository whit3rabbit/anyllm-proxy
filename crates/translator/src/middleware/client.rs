// Minimal reqwest forwarding client for the middleware layer.
// No retry logic; users can add their own Tower retry layer or use the proxy crate.

use reqwest::Client;
use thiserror::Error;

use crate::openai::{ChatCompletionRequest, ChatCompletionResponse};

/// Errors from the forwarding client.
#[derive(Error, Debug)]
pub enum ForwardingError {
    #[error("HTTP request failed: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("JSON deserialization failed: {0}")]
    Json(#[from] serde_json::Error),

    /// Backend returned a non-2xx status with a body we could read.
    #[error("backend returned HTTP {status}: {body}")]
    ApiError { status: u16, body: String },
}

impl ForwardingError {
    /// Extract (message, HTTP status) if this is an API error.
    pub fn api_error_details(&self) -> Option<(&str, u16)> {
        match self {
            Self::ApiError { status, body } => Some((body.as_str(), *status)),
            _ => None,
        }
    }
}

/// Minimal HTTP client that forwards OpenAI Chat Completions requests to a backend URL.
#[derive(Clone)]
pub struct ForwardingClient {
    client: Client,
    chat_completions_url: String,
    api_key: String,
}

impl ForwardingClient {
    pub fn new(backend_url: &str, api_key: &str) -> Self {
        let base = backend_url.trim_end_matches('/');
        Self {
            client: Client::new(),
            chat_completions_url: format!("{base}/v1/chat/completions"),
            api_key: api_key.to_string(),
        }
    }

    /// Send a non-streaming chat completion request.
    pub async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<(ChatCompletionResponse, u16), ForwardingError> {
        let response = self
            .client
            .post(&self.chat_completions_url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ForwardingError::ApiError { status, body });
        }

        let body = response.text().await?;
        let resp: ChatCompletionResponse = serde_json::from_str(&body)?;
        Ok((resp, status))
    }

    /// Send a streaming chat completion request, returning the raw response for SSE parsing.
    pub async fn chat_completion_stream(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<reqwest::Response, ForwardingError> {
        let response = self
            .client
            .post(&self.chat_completions_url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ForwardingError::ApiError { status, body });
        }

        Ok(response)
    }
}
