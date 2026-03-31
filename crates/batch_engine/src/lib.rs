// crates/batch_engine/src/lib.rs
//! Batch orchestration engine: job queue, file storage, webhook delivery.
//!
//! HTTP-agnostic. The proxy crate wires this into axum routes.

pub mod db;
pub mod engine;
pub mod error;
pub mod file_store;
pub mod job;
pub mod queue;
pub mod validation;
pub mod webhook;

pub use engine::BatchEngine;
pub use error::{EngineError, QueueError};
pub use job::*;
pub use validation::{validate_jsonl, ValidatedJsonl};
