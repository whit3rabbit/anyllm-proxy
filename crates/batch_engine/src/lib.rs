// crates/batch_engine/src/lib.rs
//! Batch orchestration engine: job queue, file storage, webhook delivery.
//!
//! HTTP-agnostic. The proxy crate wires this into axum routes.

/// Database access layer for batch jobs and webhook tasks.
pub mod db;
/// The core orchestrator orchestrating background job processing loops.
pub mod engine;
/// Engine and queue error definitions.
pub mod error;
/// Local file storage handling JSONL batch request and response payloads.
pub mod file_store;
/// Batch job structs, states, and metadata types.
pub mod job;
/// FIFO/Priority job queues (SQLite or in-memory).
pub mod queue;
/// Request validation helpers for batch JSONL lines.
pub mod validation;
/// Outbound webhook notification queues and delivery workers.
pub mod webhook;

pub use engine::BatchEngine;
pub use error::{EngineError, QueueError};
pub use job::*;
pub use validation::{validate_jsonl, ValidatedJsonl};
