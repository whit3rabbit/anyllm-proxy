//! Batch API HTTP handlers. Types and logic live in anyllm_batch_engine.

pub mod anthropic_batch;
pub mod db;
pub mod openai_batch_client;
pub mod routes;

// Re-export engine types for handler use.
pub use anyllm_batch_engine::job::{BatchId, BatchJob, BatchStatus, RequestCounts};
pub use anyllm_batch_engine::validation::{validate_jsonl, ValidatedJsonl};
pub use anyllm_batch_engine::BatchEngine;
