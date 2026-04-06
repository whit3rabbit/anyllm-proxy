// crates/batch_engine/src/error.rs
//! Engine and queue error types.

use thiserror::Error;

/// Top-level error type returned by `BatchEngine` operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("batch not found: {0}")]
    NotFound(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("queue error: {0}")]
    Queue(#[from] QueueError),

    #[error("backend error: {0}")]
    Backend(String),
}

/// Storage-layer errors returned by `JobQueue` and `WebhookQueue` implementations.
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("not found")]
    NotFound,

    #[error("already claimed")]
    AlreadyClaimed,

    #[error("storage error: {0}")]
    Storage(String),
}

impl From<rusqlite::Error> for QueueError {
    fn from(e: rusqlite::Error) -> Self {
        QueueError::Storage(e.to_string())
    }
}
