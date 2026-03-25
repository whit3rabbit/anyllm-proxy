//! Error types for the client crate.

use anyllm_translate::TranslateError;

/// Errors from the high-level [`Client`](crate::Client).
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Translation failed (e.g., unsupported feature with `LossyBehavior::Error`).
    #[error("translation error: {0}")]
    Translation(#[from] TranslateError),

    /// HTTP transport failure (DNS, TLS, connection refused, timeout).
    #[error("request failed: {0}")]
    Transport(#[from] reqwest::Error),

    /// Backend returned a non-2xx status with an error body.
    #[error("API error ({status}): {message}")]
    ApiError {
        status: u16,
        message: String,
        body: String,
    },

    /// Response body could not be deserialized.
    #[error("response deserialization failed: {0}")]
    Deserialization(String),

    /// SSE stream error.
    #[error("SSE stream error: {0}")]
    Sse(#[from] crate::sse::SseError),
}

impl ClientError {
    /// HTTP status code for API errors, or 500 for transport/deserialization errors.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::ApiError { status, .. } => *status,
            _ => 500,
        }
    }
}
