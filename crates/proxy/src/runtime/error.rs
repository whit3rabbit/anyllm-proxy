//! Runtime-specific error type (no axum response types).

use crate::backend::BackendError;
use crate::config::BackendKind;
use std::fmt;

/// Runtime errors that do not expose axum response types.
#[derive(Debug)]
pub enum ChatCompletionError {
    InvalidRequest(String),
    Translation(anyllm_translate::TranslateError),
    Routing(String),
    UnsupportedBackend {
        backend_name: String,
        backend_kind: BackendKind,
    },
    Backend(BackendError),
    StreamRead(String),
    StreamParse(String),
    StreamBufferOverflow,
    StreamTimeout,
}

impl ChatCompletionError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::InvalidRequest(_) | Self::Translation(_) | Self::UnsupportedBackend { .. } => 400,
            Self::Routing(_) => 429,
            Self::Backend(e) => e.status_code(),
            Self::StreamRead(_)
            | Self::StreamParse(_)
            | Self::StreamBufferOverflow
            | Self::StreamTimeout => 502,
        }
    }
}

impl fmt::Display for ChatCompletionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(msg) => write!(f, "{msg}"),
            Self::Translation(e) => write!(f, "{e}"),
            Self::Routing(msg) => write!(f, "{msg}"),
            Self::UnsupportedBackend {
                backend_name,
                backend_kind,
            } => write!(
                f,
                "backend '{backend_name}' ({backend_kind:?}) does not support Chat Completions runtime"
            ),
            Self::Backend(e) => write!(f, "{e}"),
            Self::StreamRead(e) => write!(f, "stream read error: {e}"),
            Self::StreamParse(e) => write!(f, "stream parse error: {e}"),
            Self::StreamBufferOverflow => write!(f, "SSE buffer exceeded maximum size"),
            Self::StreamTimeout => write!(f, "stream exceeded wall-clock timeout"),
        }
    }
}

impl std::error::Error for ChatCompletionError {}

impl From<anyllm_translate::TranslateError> for ChatCompletionError {
    fn from(e: anyllm_translate::TranslateError) -> Self {
        Self::Translation(e)
    }
}

impl From<BackendError> for ChatCompletionError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
