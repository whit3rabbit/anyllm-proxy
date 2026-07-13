//! Error types. Every `Err` in the pipeline results in fail-open (forward the original).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OptimizeError {
    #[error("edit validation failed: {0}")]
    Edit(#[from] crate::edit::EditError),
    #[error("scoring failed: {0}")]
    Score(#[from] ScoreError),
    #[error("invalid IR: {0}")]
    Ir(String),
    #[error("cost estimation failed: {0}")]
    Cost(String),
}

#[derive(Debug, Error)]
pub enum ScoreError {
    #[error("scorer deadline exceeded")]
    Deadline,
    #[error("scorer backend error: {0}")]
    Backend(String),
    #[error("input too large for scorer")]
    InputTooLarge,
}
