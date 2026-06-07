---
title: "anyllm_batch_engine"
description: "Reference for the HTTP-agnostic batch orchestration crate, including job types, submission flow, and JSONL validation."
---

Source files: `crates/batch_engine/src/lib.rs`, `crates/batch_engine/src/engine.rs`, `crates/batch_engine/src/job.rs`, `crates/batch_engine/src/validation.rs`

## Import Path

```rust
use anyllm_batch_engine::{
    BatchEngine, BatchId, ItemId, BatchJob, BatchItem, BatchStatus, ItemStatus,
    RequestCounts, BatchSubmission, SubmissionItem, ExecutionMode, SourceFormat,
    validate_jsonl, ValidatedJsonl,
};
```

## `BatchEngine`

```rust
pub struct BatchEngine<Q: JobQueue, W: WebhookQueue> {
    pub queue: Arc<Q>,
    pub file_store: FileStore,
    pub webhook_queue: Arc<W>,
    pub global_webhook_urls: Vec<String>,
    pub webhook_signing_secret: Option<String>,
}

pub async fn submit(&self, submission: BatchSubmission) -> Result<BatchJob, EngineError>
pub async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, EngineError>
pub async fn list(
    &self,
    key_id: Option<i64>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Vec<BatchJob>, EngineError>
pub async fn cancel(&self, id: &BatchId) -> Result<BatchJob, EngineError>
pub async fn get_items(&self, id: &BatchId) -> Result<Vec<BatchItem>, EngineError>
```

### Method summary

| Method | Parameters | Return type | Description |
|---|---|---|---|
| `submit` | `submission: BatchSubmission` | `Result<BatchJob, EngineError>` | Validates file existence, builds job and items, enqueues work, emits `batch.queued`. |
| `get` | `id: &BatchId` | `Result<Option<BatchJob>, EngineError>` | Loads one job. |
| `list` | `key_id`, `cursor`, `limit` | `Result<Vec<BatchJob>, EngineError>` | Lists jobs for admin or user views. |
| `cancel` | `id: &BatchId` | `Result<BatchJob, EngineError>` | Cancels the job and emits `batch.cancelled` when applicable. |
| `get_items` | `id: &BatchId` | `Result<Vec<BatchItem>, EngineError>` | Loads item-level results. |

## Core Job Types

```rust
pub struct BatchId(pub String)
pub struct ItemId(pub String)

pub enum BatchStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    Expired,
}

pub enum ExecutionMode {
    Native { provider: String },
    ProxyNative,
}

pub struct BatchSubmission {
    pub items: Vec<SubmissionItem>,
    pub execution_mode: ExecutionMode,
    pub input_file_id: String,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub priority: u8,
}
```

## Validation API

```rust
pub struct ValidatedJsonl {
    pub line_count: usize,
}

pub fn validate_jsonl(
    reader: impl std::io::BufRead,
) -> Result<ValidatedJsonl, String>
```

`validate_jsonl` rejects files larger than 100 MB, longer than 50,000 non-blank lines, empty files, duplicate `custom_id` values, missing `body`, and missing `body.model`.

## Example

```rust
use anyllm_batch_engine::{validate_jsonl, BatchId};
use std::io::Cursor;

let data = br#"{"custom_id":"a","body":{"model":"gpt-4o-mini"}}"#;
let validated = validate_jsonl(Cursor::new(&data[..]))?;
let id = BatchId::new();

assert_eq!(validated.line_count, 1);
assert!(id.0.starts_with("batch_"));
# Ok::<(), String>(())
```
