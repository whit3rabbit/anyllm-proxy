# Batch Orchestration Engine - Phase 1: Core Engine Crate + Enhanced Pass-Through

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `batch_engine` crate with core types, `JobQueue` trait, SQLite implementation, and refactor existing proxy batch handlers to use it. Add cancellation, virtual key attribution, and durable webhook delivery.

**Architecture:** New `crates/batch_engine` crate owns all batch orchestration types, queue trait, and SQLite storage. Proxy batch handlers become thin HTTP adapters that parse requests, call engine methods, and format responses. The translator crate is unchanged. This phase does NOT add workers or proxy-native execution (Phase 2).

**Tech Stack:** Rust stable (1.83+, edition 2021), rusqlite 0.32, tokio 1, uuid 1, serde/serde_json 1, async-trait 0.1, thiserror 2

---

## File Structure

### New files (crates/batch_engine/)

| File | Responsibility |
|------|----------------|
| `Cargo.toml` | Crate manifest, depends on `anyllm_translate`, `rusqlite`, `serde`, `uuid`, `tokio`, `async-trait`, `thiserror`, `reqwest` |
| `src/lib.rs` | Crate root, re-exports public API |
| `src/job.rs` | Core types: `BatchId`, `ItemId`, `BatchJob`, `BatchItem`, `BatchStatus`, `ExecutionMode`, `RequestCounts`, `BatchItemRequest`, `BatchItemResult`, `SourceFormat`, `ItemStatus` |
| `src/validation.rs` | JSONL validation (migrated from `proxy/src/batch/mod.rs`) |
| `src/error.rs` | `EngineError` and `QueueError` enums |
| `src/queue/mod.rs` | `JobQueue` trait definition |
| `src/queue/sqlite.rs` | `SqliteQueue` implementation |
| `src/db.rs` | Schema initialization, migrations from old tables |
| `src/file_store.rs` | Batch file storage (upload, get metadata, get content) |
| `src/webhook/mod.rs` | `WebhookQueue` trait, `WebhookDelivery` type |
| `src/webhook/sqlite.rs` | `SqliteWebhookQueue` implementation |
| `src/webhook/dispatcher.rs` | `WebhookDispatcher` background loop |
| `src/engine.rs` | `BatchEngine` facade: submit, get, list, cancel, get_results |

### Modified files (crates/proxy/)

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `anyllm_batch_engine` dependency |
| `src/batch/mod.rs` | Remove types and validation (now in batch_engine), re-export from engine |
| `src/batch/routes.rs` | Simplify handlers to use `BatchEngine` |
| `src/batch/anthropic_batch.rs` | Simplify handlers to use `BatchEngine` |
| `src/batch/db.rs` | Remove (replaced by batch_engine/src/db.rs) |
| `src/batch/openai_batch_client.rs` | Move to batch_engine |
| `src/admin/db.rs` | Remove batch table creation (handled by batch_engine) |
| `src/server/routes.rs` | Add cancel routes, pass `BatchEngine` in `AppState` |

### Workspace root

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `"crates/batch_engine"` to workspace members |

---

## Task 1: Create batch_engine crate scaffold

**Files:**
- Create: `crates/batch_engine/Cargo.toml`
- Create: `crates/batch_engine/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create crate directory**

```bash
mkdir -p crates/batch_engine/src
```

- [ ] **Step 2: Write Cargo.toml**

```toml
# crates/batch_engine/Cargo.toml
[package]
name = "anyllm_batch_engine"
description = "Batch orchestration engine with job queue, workers, and event-driven notifications"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
anyllm_translate = { path = "../translator", version = "0.2.0" }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
async-trait = "0.1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json", "native-tls"] }
url = "2"
hmac = "0.12"
sha2 = "0.10"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
pretty_assertions = "1"
```

- [ ] **Step 3: Write lib.rs**

```rust
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
```

- [ ] **Step 4: Add to workspace**

In `Cargo.toml` (workspace root), change:

```toml
members = ["crates/translator", "crates/client", "crates/proxy"]
```

to:

```toml
members = ["crates/translator", "crates/client", "crates/batch_engine", "crates/proxy"]
```

- [ ] **Step 5: Verify scaffold compiles**

Run: `cargo check -p anyllm_batch_engine`
Expected: Errors about missing modules (expected at this stage, just verifying Cargo.toml is valid)

- [ ] **Step 6: Commit**

```bash
git add crates/batch_engine/ Cargo.toml
git commit -m "feat(batch_engine): scaffold new crate with dependencies"
```

---

## Task 2: Core types (job.rs)

**Files:**
- Create: `crates/batch_engine/src/job.rs`

- [ ] **Step 1: Write job.rs with all core types**

```rust
// crates/batch_engine/src/job.rs
//! Core batch orchestration types. HTTP-agnostic.

use serde::{Deserialize, Serialize};

/// Unique batch job identifier. Format: "batch_{uuid}".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BatchId(pub String);

/// Unique item identifier within a batch. Format: "item_{uuid}".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub String);

impl BatchId {
    pub fn new() -> Self {
        Self(format!("batch_{}", uuid::Uuid::new_v4()))
    }
}

impl ItemId {
    pub fn new() -> Self {
        Self(format!("item_{}", uuid::Uuid::new_v4()))
    }
}

impl std::fmt::Display for BatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Batch job lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    Expired,
}

impl BatchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn from_str_status(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "processing" => Self::Processing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            "expired" => Self::Expired,
            _ => Self::Failed,
        }
    }

    /// Whether this status is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Expired
        )
    }
}

/// How the batch will be executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExecutionMode {
    /// Delegate to provider's native batch API (OpenAI, Azure).
    Native { provider: String },
    /// Proxy processes items individually against the backend.
    ProxyNative,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Native { .. } => "native",
            Self::ProxyNative => "proxy_native",
        }
    }

    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Native { provider } => Some(provider),
            Self::ProxyNative => None,
        }
    }
}

/// A batch job as seen by the engine.
#[derive(Debug, Clone, Serialize)]
pub struct BatchJob {
    pub id: BatchId,
    pub status: BatchStatus,
    pub execution_mode: ExecutionMode,
    pub priority: u8,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub request_counts: RequestCounts,
    pub input_file_id: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub expires_at: String,
}

/// Counts of requests within a batch job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestCounts {
    pub total: u32,
    pub processing: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub expired: u32,
}

/// Single item within a batch.
#[derive(Debug, Clone)]
pub struct BatchItem {
    pub id: ItemId,
    pub batch_id: BatchId,
    pub custom_id: String,
    pub status: ItemStatus,
    pub request: BatchItemRequest,
    pub result: Option<BatchItemResult>,
    pub attempts: u8,
    pub max_retries: u8,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub lease_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Processing,
    Succeeded,
    Failed,
    Cancelled,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str_status(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "processing" => Self::Processing,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// The LLM request payload for a batch item.
#[derive(Debug, Clone)]
pub struct BatchItemRequest {
    pub model: String,
    pub body: serde_json::Value,
    pub source_format: SourceFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Anthropic,
    OpenAI,
}

/// Result of executing a single batch item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemResult {
    pub status_code: u16,
    pub body: serde_json::Value,
}

/// Submission request to the engine (from proxy handlers).
pub struct BatchSubmission {
    pub items: Vec<SubmissionItem>,
    pub execution_mode: ExecutionMode,
    pub input_file_id: String,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub priority: u8,
}

/// A single item in a batch submission.
pub struct SubmissionItem {
    pub custom_id: String,
    pub model: String,
    pub body: serde_json::Value,
    pub source_format: SourceFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_id_format() {
        let id = BatchId::new();
        assert!(id.0.starts_with("batch_"));
        assert_eq!(id.0.len(), 6 + 36); // "batch_" + uuid
    }

    #[test]
    fn item_id_format() {
        let id = ItemId::new();
        assert!(id.0.starts_with("item_"));
    }

    #[test]
    fn batch_status_roundtrip() {
        for status in [
            BatchStatus::Queued,
            BatchStatus::Processing,
            BatchStatus::Completed,
            BatchStatus::Failed,
            BatchStatus::Cancelling,
            BatchStatus::Cancelled,
            BatchStatus::Expired,
        ] {
            let s = status.as_str();
            let parsed = BatchStatus::from_str_status(s);
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn terminal_statuses() {
        assert!(!BatchStatus::Queued.is_terminal());
        assert!(!BatchStatus::Processing.is_terminal());
        assert!(BatchStatus::Completed.is_terminal());
        assert!(BatchStatus::Failed.is_terminal());
        assert!(BatchStatus::Cancelled.is_terminal());
        assert!(BatchStatus::Expired.is_terminal());
    }

    #[test]
    fn execution_mode_str() {
        let native = ExecutionMode::Native {
            provider: "openai".into(),
        };
        assert_eq!(native.as_str(), "native");
        assert_eq!(native.provider(), Some("openai"));

        let proxy = ExecutionMode::ProxyNative;
        assert_eq!(proxy.as_str(), "proxy_native");
        assert_eq!(proxy.provider(), None);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p anyllm_batch_engine`
Expected: Errors about missing modules (error.rs, etc.) but job.rs itself compiles

- [ ] **Step 3: Commit**

```bash
git add crates/batch_engine/src/job.rs
git commit -m "feat(batch_engine): add core types (BatchJob, BatchItem, RequestCounts)"
```

---

## Task 3: Error types (error.rs)

**Files:**
- Create: `crates/batch_engine/src/error.rs`

- [ ] **Step 1: Write error.rs**

```rust
// crates/batch_engine/src/error.rs
//! Engine and queue error types.

use thiserror::Error;

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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p anyllm_batch_engine`
Expected: Remaining module errors (queue, etc.) but error.rs compiles

- [ ] **Step 3: Commit**

```bash
git add crates/batch_engine/src/error.rs
git commit -m "feat(batch_engine): add EngineError and QueueError types"
```

---

## Task 4: JSONL validation (validation.rs)

**Files:**
- Create: `crates/batch_engine/src/validation.rs`

This is a direct migration from `crates/proxy/src/batch/mod.rs` lines 104-190, with its tests.

- [ ] **Step 1: Write validation.rs**

```rust
// crates/batch_engine/src/validation.rs
//! JSONL batch file validation.

use std::collections::HashSet;

/// Maximum number of lines in a JSONL batch file.
const MAX_LINE_COUNT: usize = 50_000;

/// Maximum file size in bytes (100 MB).
const MAX_FILE_SIZE: usize = 100 * 1024 * 1024;

/// Maximum length of a custom_id field.
const MAX_CUSTOM_ID_LEN: usize = 64;

/// Result of JSONL validation: line count on success.
#[derive(Debug)]
pub struct ValidatedJsonl {
    pub line_count: usize,
}

/// Validate a JSONL batch file.
///
/// Each line must be valid JSON with a unique `custom_id` (string, max 64 chars)
/// and a `body` object containing a `model` field. Max 50,000 lines, 100 MB.
pub fn validate_jsonl(mut reader: impl std::io::BufRead) -> Result<ValidatedJsonl, String> {
    let mut seen_ids = HashSet::new();
    let mut line_count = 0usize;
    let mut raw_line_num = 0usize;
    let mut bytes_read = 0usize;
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let n = reader
            .read_line(&mut line_buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 {
            break;
        }
        raw_line_num += 1;
        bytes_read += n;
        if bytes_read > MAX_FILE_SIZE {
            return Err(format!(
                "File exceeds maximum size of {} bytes",
                MAX_FILE_SIZE
            ));
        }

        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        line_count += 1;
        if line_count > MAX_LINE_COUNT {
            return Err(format!("File exceeds maximum of {MAX_LINE_COUNT} lines"));
        }

        let parsed: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("Line {raw_line_num}: invalid JSON: {e}"))?;

        let obj = parsed
            .as_object()
            .ok_or_else(|| format!("Line {raw_line_num}: expected JSON object"))?;

        let custom_id = obj
            .get("custom_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Line {raw_line_num}: missing or non-string 'custom_id'"))?;

        if custom_id.len() > MAX_CUSTOM_ID_LEN {
            return Err(format!(
                "Line {raw_line_num}: custom_id exceeds maximum length of {MAX_CUSTOM_ID_LEN} characters"
            ));
        }

        if !seen_ids.insert(custom_id.to_string()) {
            return Err(format!(
                "Line {raw_line_num}: duplicate custom_id '{custom_id}'"
            ));
        }

        let body = obj
            .get("body")
            .and_then(|v| v.as_object())
            .ok_or_else(|| format!("Line {raw_line_num}: missing or non-object 'body'"))?;

        if !body.contains_key("model") {
            return Err(format!("Line {raw_line_num}: body missing 'model' field"));
        }
    }

    if line_count == 0 {
        return Err("File is empty".to_string());
    }

    Ok(ValidatedJsonl { line_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn check(data: &str) -> Result<ValidatedJsonl, String> {
        validate_jsonl(BufReader::new(Cursor::new(data.as_bytes())))
    }

    fn check_bytes(data: &[u8]) -> Result<ValidatedJsonl, String> {
        validate_jsonl(BufReader::new(Cursor::new(data)))
    }

    #[test]
    fn valid_jsonl() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o", "messages": []}}
{"custom_id": "req-2", "body": {"model": "gpt-4o", "messages": []}}"#;
        let result = check(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }

    #[test]
    fn missing_custom_id() {
        let data = r#"{"body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("custom_id"));
    }

    #[test]
    fn missing_body_model() {
        let data = r#"{"custom_id": "req-1", "body": {"messages": []}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("model"));
    }

    #[test]
    fn duplicate_custom_id() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}
{"custom_id": "req-1", "body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate"));
    }

    #[test]
    fn oversized_custom_id() {
        let long_id = "a".repeat(65);
        let data = format!(r#"{{"custom_id": "{long_id}", "body": {{"model": "gpt-4o"}}}}"#);
        let result = check(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("maximum length"));
    }

    #[test]
    fn empty_file() {
        let result = check_bytes(b"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn invalid_json_line() {
        let data = b"not json at all";
        let result = check_bytes(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn blank_lines_skipped() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}

{"custom_id": "req-2", "body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }

    #[test]
    fn error_reports_absolute_line_number_with_blank_lines() {
        let data = "\n{\"custom_id\": \"ok\", \"body\": INVALID}";
        let err = check(data).unwrap_err();
        assert!(err.contains("Line 2"), "expected 'Line 2' in: {err}");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p anyllm_batch_engine validation`
Expected: All 9 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/batch_engine/src/validation.rs
git commit -m "feat(batch_engine): migrate JSONL validation from proxy crate"
```

---

## Task 5: Schema initialization and file storage (db.rs, file_store.rs)

**Files:**
- Create: `crates/batch_engine/src/db.rs`
- Create: `crates/batch_engine/src/file_store.rs`

- [ ] **Step 1: Write db.rs with new schema**

```rust
// crates/batch_engine/src/db.rs
//! SQLite schema initialization for batch_engine tables.

use rusqlite::Connection;

/// ISO 8601 timestamp for "now" in UTC.
pub fn now_iso8601() -> String {
    // Replicates the pattern used in proxy's admin/db.rs.
    // Using SystemTime to avoid chrono dependency.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Convert epoch seconds to ISO 8601. Simplified: just store epoch
    // and use SQLite's datetime() for display. But for compatibility
    // with existing code, produce a formatted string.
    let secs = now;
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Civil date from days since epoch (Howard Hinnant algorithm).
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_val = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_val = if m_val <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y_val, m_val, d, h, m, s
    )
}

/// Initialize all batch_engine tables.
pub fn init_batch_engine_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS batch_job (
            batch_id          TEXT PRIMARY KEY,
            status            TEXT NOT NULL DEFAULT 'queued',
            execution_mode    TEXT NOT NULL,
            provider          TEXT,
            provider_batch_id TEXT,
            priority          INTEGER NOT NULL DEFAULT 0,
            key_id            INTEGER,
            input_file_id     TEXT NOT NULL,
            webhook_url       TEXT,
            metadata          TEXT,
            total             INTEGER NOT NULL DEFAULT 0,
            processing        INTEGER NOT NULL DEFAULT 0,
            succeeded         INTEGER NOT NULL DEFAULT 0,
            failed            INTEGER NOT NULL DEFAULT 0,
            cancelled         INTEGER NOT NULL DEFAULT 0,
            expired           INTEGER NOT NULL DEFAULT 0,
            created_at        TEXT NOT NULL,
            started_at        TEXT,
            completed_at      TEXT,
            expires_at        TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_batch_job_dequeue
            ON batch_job(status, priority DESC, created_at ASC);

        CREATE INDEX IF NOT EXISTS idx_batch_job_key
            ON batch_job(key_id) WHERE key_id IS NOT NULL;

        CREATE INDEX IF NOT EXISTS idx_batch_job_native
            ON batch_job(status, execution_mode)
            WHERE execution_mode = 'native' AND status = 'processing';

        CREATE TABLE IF NOT EXISTS batch_item (
            item_id          TEXT PRIMARY KEY,
            batch_id         TEXT NOT NULL REFERENCES batch_job(batch_id),
            custom_id        TEXT NOT NULL,
            status           TEXT NOT NULL DEFAULT 'pending',
            model            TEXT NOT NULL,
            request_body     TEXT NOT NULL,
            source_format    TEXT NOT NULL,
            result_status    INTEGER,
            result_body      TEXT,
            attempts         INTEGER NOT NULL DEFAULT 0,
            max_retries      INTEGER NOT NULL DEFAULT 3,
            last_error       TEXT,
            idempotency_key  TEXT,
            next_retry_at    TEXT,
            lease_id         TEXT,
            lease_expires_at TEXT,
            created_at       TEXT NOT NULL,
            completed_at     TEXT,
            UNIQUE(batch_id, custom_id)
        );

        CREATE INDEX IF NOT EXISTS idx_batch_item_claim
            ON batch_item(status, next_retry_at, created_at)
            WHERE status IN ('pending', 'failed');

        CREATE INDEX IF NOT EXISTS idx_batch_item_batch
            ON batch_item(batch_id, status);

        CREATE INDEX IF NOT EXISTS idx_batch_item_lease
            ON batch_item(lease_expires_at)
            WHERE lease_id IS NOT NULL;

        CREATE TABLE IF NOT EXISTS batch_dead_letter (
            item_id      TEXT PRIMARY KEY,
            batch_id     TEXT NOT NULL,
            custom_id    TEXT NOT NULL,
            request_body TEXT NOT NULL,
            last_error   TEXT,
            attempts     INTEGER NOT NULL,
            failed_at    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS batch_file (
            file_id    TEXT PRIMARY KEY,
            key_id     INTEGER,
            purpose    TEXT NOT NULL DEFAULT 'batch',
            filename   TEXT,
            byte_size  INTEGER NOT NULL,
            line_count INTEGER NOT NULL,
            content    BLOB NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS anthropic_batch_map (
            our_batch_id    TEXT PRIMARY KEY,
            engine_batch_id TEXT NOT NULL,
            model           TEXT NOT NULL,
            created_at      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS webhook_delivery (
            delivery_id    TEXT PRIMARY KEY,
            event_id       TEXT NOT NULL,
            batch_id       TEXT NOT NULL,
            url            TEXT NOT NULL,
            payload        TEXT NOT NULL,
            signing_secret TEXT,
            status         TEXT NOT NULL DEFAULT 'pending',
            attempts       INTEGER NOT NULL DEFAULT 0,
            max_retries    INTEGER NOT NULL DEFAULT 3,
            next_retry_at  TEXT,
            lease_id       TEXT,
            lease_expires_at TEXT,
            created_at     TEXT NOT NULL,
            delivered_at   TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_webhook_claim
            ON webhook_delivery(status, next_retry_at)
            WHERE status IN ('pending', 'processing');

        CREATE TABLE IF NOT EXISTS batch_event_log (
            event_id   TEXT PRIMARY KEY,
            batch_id   TEXT NOT NULL,
            sequence   INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            payload    TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(batch_id, sequence)
        );

        CREATE INDEX IF NOT EXISTS idx_event_log_batch
            ON batch_event_log(batch_id, sequence);
        ",
    )
}

/// Migrate old batch tables (from proxy's admin/db.rs schema) if they exist.
/// Renames them to _v1 suffix. Safe to call multiple times.
pub fn migrate_old_tables(conn: &Connection) -> rusqlite::Result<()> {
    // Check if old-schema batch_job exists (has `backend_name` column).
    let has_old_batch_job: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('batch_job') WHERE name = 'backend_name'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if has_old_batch_job {
        conn.execute_batch(
            "ALTER TABLE batch_job RENAME TO batch_job_v1;
             ALTER TABLE batch_file RENAME TO batch_file_v1;",
        )?;
        tracing::info!("migrated old batch_job and batch_file tables to _v1");
    }

    // Migrate old anthropic_batch_map if it has openai_batch_id column.
    let has_old_abm: bool = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('anthropic_batch_map') WHERE name = 'openai_batch_id'",
        )
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if has_old_abm {
        conn.execute_batch("ALTER TABLE anthropic_batch_map RENAME TO anthropic_batch_map_v1;")?;
        tracing::info!("migrated old anthropic_batch_map to _v1");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_tables_succeeds() {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();

        // Verify tables exist by querying them.
        conn.execute("SELECT count(*) FROM batch_job", []).unwrap();
        conn.execute("SELECT count(*) FROM batch_item", []).unwrap();
        conn.execute("SELECT count(*) FROM batch_file", []).unwrap();
        conn.execute("SELECT count(*) FROM webhook_delivery", [])
            .unwrap();
        conn.execute("SELECT count(*) FROM batch_event_log", [])
            .unwrap();
    }

    #[test]
    fn init_tables_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        init_batch_engine_tables(&conn).unwrap(); // no error
    }

    #[test]
    fn now_iso8601_format() {
        let ts = now_iso8601();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
    }
}
```

- [ ] **Step 2: Write file_store.rs**

```rust
// crates/batch_engine/src/file_store.rs
//! Batch file storage (upload, metadata, content retrieval).

use crate::db::now_iso8601;
use crate::error::QueueError;
use rusqlite::{params, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Batch file metadata (without content blob).
#[derive(Debug, Clone)]
pub struct BatchFileMeta {
    pub file_id: String,
    pub byte_size: i64,
    pub line_count: i64,
    pub filename: Option<String>,
    pub created_at: String,
}

/// Manages batch file storage in SQLite.
#[derive(Clone)]
pub struct FileStore {
    db: Arc<Mutex<Connection>>,
}

impl FileStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Store a batch file. Returns the file_id.
    pub async fn insert(
        &self,
        file_id: &str,
        key_id: Option<i64>,
        filename: Option<&str>,
        content: &[u8],
        line_count: i64,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();
        let key_id = key_id;
        let filename = filename.map(|s| s.to_string());
        let content = content.to_vec();
        let byte_size = content.len() as i64;

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "INSERT INTO batch_file (file_id, key_id, purpose, filename, byte_size, line_count, content, created_at)
                 VALUES (?1, ?2, 'batch', ?3, ?4, ?5, ?6, ?7)",
                params![file_id, key_id, filename, byte_size, line_count, content, now_iso8601()],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    /// Get file metadata (without content).
    pub async fn get_meta(&self, file_id: &str) -> Result<Option<BatchFileMeta>, QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT file_id, byte_size, line_count, filename, created_at FROM batch_file WHERE file_id = ?1",
            )?;
            let mut rows = stmt.query(params![file_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(BatchFileMeta {
                    file_id: row.get(0)?,
                    byte_size: row.get(1)?,
                    line_count: row.get(2)?,
                    filename: row.get(3)?,
                    created_at: row.get(4)?,
                }))
            } else {
                Ok(None)
            }
        })
        .await
        .unwrap()
    }

    /// Get file content (raw JSONL bytes).
    pub async fn get_content(&self, file_id: &str) -> Result<Option<Vec<u8>>, QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt =
                conn.prepare("SELECT content FROM batch_file WHERE file_id = ?1")?;
            let mut rows = stmt.query(params![file_id])?;
            if let Some(row) = rows.next()? {
                let content: Vec<u8> = row.get(0)?;
                Ok(Some(content))
            } else {
                Ok(None)
            }
        })
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_store() -> FileStore {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        FileStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn insert_and_get_meta() {
        let store = test_store().await;
        store
            .insert("file-abc", None, Some("test.jsonl"), b"line1\nline2", 2)
            .await
            .unwrap();

        let meta = store.get_meta("file-abc").await.unwrap().unwrap();
        assert_eq!(meta.file_id, "file-abc");
        assert_eq!(meta.byte_size, 11);
        assert_eq!(meta.line_count, 2);
        assert_eq!(meta.filename.as_deref(), Some("test.jsonl"));
    }

    #[tokio::test]
    async fn get_content_roundtrip() {
        let store = test_store().await;
        let data = b"test content bytes";
        store
            .insert("file-xyz", None, None, data, 1)
            .await
            .unwrap();

        let content = store.get_content("file-xyz").await.unwrap().unwrap();
        assert_eq!(content, data);
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = test_store().await;
        assert!(store.get_meta("file-nope").await.unwrap().is_none());
        assert!(store.get_content("file-nope").await.unwrap().is_none());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_batch_engine`
Expected: All tests in db.rs, file_store.rs, validation.rs, and job.rs pass

- [ ] **Step 4: Commit**

```bash
git add crates/batch_engine/src/db.rs crates/batch_engine/src/file_store.rs
git commit -m "feat(batch_engine): add schema init, migration, and file storage"
```

---

## Task 6: JobQueue trait and SqliteQueue (queue/)

**Files:**
- Create: `crates/batch_engine/src/queue/mod.rs`
- Create: `crates/batch_engine/src/queue/sqlite.rs`

- [ ] **Step 1: Write queue/mod.rs with the trait**

```rust
// crates/batch_engine/src/queue/mod.rs
//! JobQueue trait and implementations.

pub mod sqlite;

use crate::error::QueueError;
use crate::job::*;
use async_trait::async_trait;
use std::time::Duration;

/// A job that has been claimed by a worker. Holds lease metadata.
#[derive(Debug)]
pub struct LeasedItem {
    pub item: BatchItem,
    pub batch_id: BatchId,
    pub lease_id: String,
    pub lease_expires_at: String,
}

/// Core queue abstraction. All methods are async + Send.
#[async_trait]
pub trait JobQueue: Send + Sync + 'static {
    // -- Job lifecycle --
    async fn enqueue(&self, job: &BatchJob, items: &[BatchItem]) -> Result<(), QueueError>;
    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError>;
    async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, QueueError>;
    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, QueueError>;

    // -- Item-level operations (proxy-native path, Phase 2) --
    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError>;
    async fn complete_item(
        &self,
        id: &ItemId,
        result: BatchItemResult,
    ) -> Result<(), QueueError>;
    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError>;
    async fn schedule_retry(
        &self,
        id: &ItemId,
        delay: Duration,
        error: &str,
    ) -> Result<(), QueueError>;
    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError>;

    // -- Batch completion --
    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError>;
    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError>;

    // -- Native batch support --
    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError>;

    // -- Lease management --
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;

    // -- Progress --
    async fn update_progress(
        &self,
        id: &BatchId,
        counts: &RequestCounts,
    ) -> Result<(), QueueError>;

    // -- Items query --
    async fn get_items(&self, batch_id: &BatchId) -> Result<Vec<BatchItem>, QueueError>;
}
```

- [ ] **Step 2: Write queue/sqlite.rs**

```rust
// crates/batch_engine/src/queue/sqlite.rs
//! SQLite-backed JobQueue implementation.

use super::{JobQueue, LeasedItem};
use crate::db::now_iso8601;
use crate::error::QueueError;
use crate::job::*;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// SQLite-backed job queue. Suitable for single-instance deployments.
#[derive(Clone)]
pub struct SqliteQueue {
    db: Arc<Mutex<Connection>>,
}

impl SqliteQueue {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl JobQueue for SqliteQueue {
    async fn enqueue(&self, job: &BatchJob, items: &[BatchItem]) -> Result<(), QueueError> {
        let db = self.db.clone();
        let job = job.clone();
        let items: Vec<BatchItem> = items.to_vec();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let tx = conn.unchecked_transaction()?;

            tx.execute(
                "INSERT INTO batch_job (batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata, total, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    job.id.0,
                    job.status.as_str(),
                    job.execution_mode.as_str(),
                    job.execution_mode.provider(),
                    job.priority,
                    job.key_id,
                    job.input_file_id,
                    job.webhook_url,
                    job.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default()),
                    job.request_counts.total,
                    job.created_at,
                    job.expires_at,
                ],
            )?;

            for item in &items {
                let body_str = serde_json::to_string(&item.request.body)
                    .map_err(|e| QueueError::Storage(e.to_string()))?;
                tx.execute(
                    "INSERT INTO batch_item (item_id, batch_id, custom_id, status, model,
                        request_body, source_format, max_retries, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        item.id.0,
                        item.batch_id.0,
                        item.custom_id,
                        item.status.as_str(),
                        item.request.model,
                        body_str,
                        serde_json::to_string(&item.request.source_format)
                            .unwrap_or_else(|_| "\"openai\"".to_string()),
                        item.max_retries,
                        item.created_at,
                    ],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            row_to_job(&conn, &id)
        })
        .await
        .unwrap()
    }

    async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, QueueError> {
        let db = self.db.clone();
        let cursor = cursor.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut sql = String::from(
                "SELECT batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata,
                    total, processing, succeeded, failed, cancelled, expired,
                    created_at, started_at, completed_at, expires_at
                 FROM batch_job WHERE 1=1",
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(kid) = key_id {
                sql.push_str(" AND key_id = ?");
                param_values.push(Box::new(kid));
            }
            if let Some(ref c) = cursor {
                sql.push_str(" AND created_at < (SELECT created_at FROM batch_job WHERE batch_id = ?)");
                param_values.push(Box::new(c.clone()));
            }
            sql.push_str(" ORDER BY created_at DESC LIMIT ?");
            param_values.push(Box::new(limit));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_refs.as_slice(), |row| {
                Ok(batch_job_from_row(row))
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(QueueError::from)
        })
        .await
        .unwrap()
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt =
                conn.prepare("SELECT status FROM batch_job WHERE batch_id = ?1")?;
            let status_str: Option<String> = stmt
                .query_row(params![id], |row| row.get(0))
                .ok();

            let Some(status_str) = status_str else {
                return Err(QueueError::NotFound);
            };

            let current = BatchStatus::from_str_status(&status_str);
            let new_status = match current {
                BatchStatus::Queued => BatchStatus::Cancelled,
                BatchStatus::Processing => BatchStatus::Cancelling,
                other if other.is_terminal() => return Ok(other),
                _ => BatchStatus::Cancelled,
            };

            conn.execute(
                "UPDATE batch_job SET status = ?1, completed_at = CASE WHEN ?1 = 'cancelled' THEN ?2 ELSE completed_at END
                 WHERE batch_id = ?3",
                params![new_status.as_str(), now_iso8601(), id],
            )?;

            // If directly cancelled (was queued), cancel all pending items.
            if new_status == BatchStatus::Cancelled {
                conn.execute(
                    "UPDATE batch_item SET status = 'cancelled' WHERE batch_id = ?1 AND status = 'pending'",
                    params![id],
                )?;
            }

            Ok(new_status)
        })
        .await
        .unwrap()
    }

    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let lease_id = format!("lease_{}", uuid::Uuid::new_v4());
            let now = now_iso8601();
            // Lease for 120 seconds.
            let lease_expires = {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 120;
                // Reuse now_iso8601 logic is not ideal; for the claim query
                // we just need a comparable timestamp.
                format_epoch_iso8601(secs)
            };

            let result = conn.query_row(
                "UPDATE batch_item
                 SET status = 'processing',
                     lease_id = ?1,
                     lease_expires_at = ?2,
                     attempts = attempts + 1
                 WHERE item_id = (
                     SELECT bi.item_id
                     FROM batch_item bi
                     JOIN batch_job bj ON bi.batch_id = bj.batch_id
                     WHERE bi.status IN ('pending')
                       AND (bi.next_retry_at IS NULL OR bi.next_retry_at <= ?3)
                       AND bj.status IN ('queued', 'processing')
                       AND bj.execution_mode = 'proxy_native'
                     ORDER BY bj.priority DESC, bi.created_at ASC
                     LIMIT 1
                 )
                 RETURNING item_id, batch_id, custom_id, status, model, request_body,
                           source_format, result_status, result_body, attempts,
                           max_retries, last_error, next_retry_at, lease_id,
                           lease_expires_at, idempotency_key, created_at, completed_at",
                params![lease_id, lease_expires, now],
                |row| {
                    let item = batch_item_from_row(row);
                    Ok(LeasedItem {
                        batch_id: item.batch_id.clone(),
                        lease_id: item.lease_id.clone().unwrap_or_default(),
                        lease_expires_at: item.lease_expires_at.clone().unwrap_or_default(),
                        item,
                    })
                },
            );

            match result {
                Ok(leased) => {
                    // Transition parent job to processing if still queued.
                    conn.execute(
                        "UPDATE batch_job SET status = 'processing', started_at = ?1
                         WHERE batch_id = ?2 AND status = 'queued'",
                        params![now, leased.batch_id.0],
                    )?;
                    Ok(Some(leased))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(QueueError::from(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn complete_item(
        &self,
        id: &ItemId,
        result: BatchItemResult,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let result_body = serde_json::to_string(&result.body)
            .map_err(|e| QueueError::Storage(e.to_string()))?;
        let status_code = result.status_code;

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'succeeded', result_status = ?1,
                    result_body = ?2, lease_id = NULL, lease_expires_at = NULL,
                    completed_at = ?3
                 WHERE item_id = ?4",
                params![status_code, result_body, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let error = error.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'failed', last_error = ?1,
                    lease_id = NULL, lease_expires_at = NULL, completed_at = ?2
                 WHERE item_id = ?3",
                params![error, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn schedule_retry(
        &self,
        id: &ItemId,
        delay: Duration,
        error: &str,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let error = error.to_string();
        let retry_at = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + delay.as_secs();
            format_epoch_iso8601(secs)
        };

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'pending', last_error = ?1,
                    next_retry_at = ?2, lease_id = NULL, lease_expires_at = NULL
                 WHERE item_id = ?3",
                params![error, retry_at, id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "INSERT OR IGNORE INTO batch_dead_letter (item_id, batch_id, custom_id, request_body, last_error, attempts, failed_at)
                 SELECT item_id, batch_id, custom_id, request_body, last_error, attempts, ?1
                 FROM batch_item WHERE item_id = ?2",
                params![now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM batch_item
                 WHERE batch_id = ?1 AND status NOT IN ('succeeded', 'failed', 'cancelled')",
                params![id],
                |row| row.get(0),
            )?;
            Ok(count == 0)
        })
        .await
        .unwrap()
    }

    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            // Count final item states.
            let (succeeded, failed, cancelled): (i64, i64, i64) = conn.query_row(
                "SELECT
                    COUNT(CASE WHEN status = 'succeeded' THEN 1 END),
                    COUNT(CASE WHEN status = 'failed' THEN 1 END),
                    COUNT(CASE WHEN status = 'cancelled' THEN 1 END)
                 FROM batch_item WHERE batch_id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;

            conn.execute(
                "UPDATE batch_job SET status = 'completed',
                    succeeded = ?1, failed = ?2, cancelled = ?3,
                    processing = 0, completed_at = ?4
                 WHERE batch_id = ?5",
                params![succeeded, failed, cancelled, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata,
                    total, processing, succeeded, failed, cancelled, expired,
                    created_at, started_at, completed_at, expires_at
                 FROM batch_job
                 WHERE execution_mode = 'native' AND status = 'processing'",
            )?;
            let rows = stmt.query_map([], |row| Ok(batch_job_from_row(row)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(QueueError::from)
        })
        .await
        .unwrap()
    }

    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let now = now_iso8601();
            let count = conn.execute(
                "UPDATE batch_item SET status = 'pending', lease_id = NULL, lease_expires_at = NULL
                 WHERE lease_id IS NOT NULL AND lease_expires_at < ?1 AND status = 'processing'",
                params![now],
            )?;
            if count > 0 {
                tracing::warn!(count, "reclaimed expired item leases");
            }
            Ok(count as u32)
        })
        .await
        .unwrap()
    }

    async fn update_progress(
        &self,
        id: &BatchId,
        counts: &RequestCounts,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let counts = counts.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_job SET
                    processing = ?1, succeeded = ?2, failed = ?3,
                    cancelled = ?4, expired = ?5
                 WHERE batch_id = ?6",
                params![
                    counts.processing,
                    counts.succeeded,
                    counts.failed,
                    counts.cancelled,
                    counts.expired,
                    id,
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_items(&self, batch_id: &BatchId) -> Result<Vec<BatchItem>, QueueError> {
        let db = self.db.clone();
        let batch_id = batch_id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT item_id, batch_id, custom_id, status, model, request_body,
                    source_format, result_status, result_body, attempts,
                    max_retries, last_error, next_retry_at, lease_id,
                    lease_expires_at, idempotency_key, created_at, completed_at
                 FROM batch_item WHERE batch_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![batch_id], |row| Ok(batch_item_from_row(row)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(QueueError::from)
        })
        .await
        .unwrap()
    }
}

// -- Row mappers --

fn batch_job_from_row(row: &rusqlite::Row) -> BatchJob {
    let status_str: String = row.get(1).unwrap_or_default();
    let exec_mode_str: String = row.get(2).unwrap_or_default();
    let provider: Option<String> = row.get(3).unwrap_or(None);
    let metadata_str: Option<String> = row.get(8).unwrap_or(None);

    let execution_mode = match exec_mode_str.as_str() {
        "native" => ExecutionMode::Native {
            provider: provider.unwrap_or_else(|| "unknown".into()),
        },
        _ => ExecutionMode::ProxyNative,
    };

    BatchJob {
        id: BatchId(row.get(0).unwrap_or_default()),
        status: BatchStatus::from_str_status(&status_str),
        execution_mode,
        priority: row.get::<_, i64>(4).unwrap_or(0) as u8,
        key_id: row.get(5).unwrap_or(None),
        input_file_id: row.get(6).unwrap_or_default(),
        webhook_url: row.get(7).unwrap_or(None),
        metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        request_counts: RequestCounts {
            total: row.get::<_, i64>(9).unwrap_or(0) as u32,
            processing: row.get::<_, i64>(10).unwrap_or(0) as u32,
            succeeded: row.get::<_, i64>(11).unwrap_or(0) as u32,
            failed: row.get::<_, i64>(12).unwrap_or(0) as u32,
            cancelled: row.get::<_, i64>(13).unwrap_or(0) as u32,
            expired: row.get::<_, i64>(14).unwrap_or(0) as u32,
        },
        created_at: row.get(15).unwrap_or_default(),
        started_at: row.get(16).unwrap_or(None),
        completed_at: row.get(17).unwrap_or(None),
        expires_at: row.get(18).unwrap_or_default(),
    }
}

fn batch_item_from_row(row: &rusqlite::Row) -> BatchItem {
    let status_str: String = row.get(3).unwrap_or_default();
    let model: String = row.get(4).unwrap_or_default();
    let body_str: String = row.get(5).unwrap_or_default();
    let source_fmt_str: String = row.get(6).unwrap_or_default();
    let result_status: Option<i64> = row.get(7).unwrap_or(None);
    let result_body_str: Option<String> = row.get(8).unwrap_or(None);

    let source_format = serde_json::from_str::<SourceFormat>(&source_fmt_str)
        .unwrap_or(SourceFormat::OpenAI);

    let body = serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

    let result = match (result_status, result_body_str) {
        (Some(code), Some(body_s)) => {
            let body_val = serde_json::from_str(&body_s).unwrap_or(serde_json::Value::Null);
            Some(BatchItemResult {
                status_code: code as u16,
                body: body_val,
            })
        }
        _ => None,
    };

    BatchItem {
        id: ItemId(row.get(0).unwrap_or_default()),
        batch_id: BatchId(row.get(1).unwrap_or_default()),
        custom_id: row.get(2).unwrap_or_default(),
        status: ItemStatus::from_str_status(&status_str),
        request: BatchItemRequest {
            model,
            body,
            source_format,
        },
        result,
        attempts: row.get::<_, i64>(9).unwrap_or(0) as u8,
        max_retries: row.get::<_, i64>(10).unwrap_or(3) as u8,
        last_error: row.get(11).unwrap_or(None),
        next_retry_at: row.get(12).unwrap_or(None),
        lease_id: row.get(13).unwrap_or(None),
        lease_expires_at: row.get(14).unwrap_or(None),
        idempotency_key: row.get(15).unwrap_or(None),
        created_at: row.get(16).unwrap_or_default(),
        completed_at: row.get(17).unwrap_or(None),
    }
}

fn row_to_job(conn: &Connection, batch_id: &str) -> Result<Option<BatchJob>, QueueError> {
    let mut stmt = conn.prepare(
        "SELECT batch_id, status, execution_mode, provider, priority,
            key_id, input_file_id, webhook_url, metadata,
            total, processing, succeeded, failed, cancelled, expired,
            created_at, started_at, completed_at, expires_at
         FROM batch_job WHERE batch_id = ?1",
    )?;
    let mut rows = stmt.query(params![batch_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(batch_job_from_row(row)))
    } else {
        Ok(None)
    }
}

/// Convert epoch seconds to ISO 8601 string.
pub(crate) fn format_epoch_iso8601(secs: u64) -> String {
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_val = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_val = if m_val <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y_val, m_val, d, h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_queue() -> SqliteQueue {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        SqliteQueue::new(Arc::new(Mutex::new(conn)))
    }

    fn make_job(id: &str) -> (BatchJob, Vec<BatchItem>) {
        let batch_id = BatchId(id.into());
        let now = crate::db::now_iso8601();
        let job = BatchJob {
            id: batch_id.clone(),
            status: BatchStatus::Queued,
            execution_mode: ExecutionMode::ProxyNative,
            priority: 0,
            key_id: None,
            input_file_id: "file-test".into(),
            webhook_url: None,
            metadata: None,
            request_counts: RequestCounts {
                total: 2,
                ..Default::default()
            },
            created_at: now.clone(),
            started_at: None,
            completed_at: None,
            expires_at: now.clone(),
        };
        let items = vec![
            BatchItem {
                id: ItemId(format!("{id}_item_1")),
                batch_id: batch_id.clone(),
                custom_id: "req-1".into(),
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: "gpt-4o".into(),
                    body: serde_json::json!({"messages": []}),
                    source_format: SourceFormat::OpenAI,
                },
                result: None,
                attempts: 0,
                max_retries: 3,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            },
            BatchItem {
                id: ItemId(format!("{id}_item_2")),
                batch_id: batch_id.clone(),
                custom_id: "req-2".into(),
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: "gpt-4o".into(),
                    body: serde_json::json!({"messages": []}),
                    source_format: SourceFormat::OpenAI,
                },
                result: None,
                attempts: 0,
                max_retries: 3,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            },
        ];
        (job, items)
    }

    #[tokio::test]
    async fn enqueue_and_get() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_test1");
        q.enqueue(&job, &items).await.unwrap();

        let fetched = q.get(&BatchId("batch_test1".into())).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.status, BatchStatus::Queued);
        assert_eq!(fetched.request_counts.total, 2);
    }

    #[tokio::test]
    async fn get_nonexistent() {
        let q = test_queue().await;
        let fetched = q.get(&BatchId("nope".into())).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn cancel_queued_job() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_cancel");
        q.enqueue(&job, &items).await.unwrap();

        let status = q.cancel(&BatchId("batch_cancel".into())).await.unwrap();
        assert_eq!(status, BatchStatus::Cancelled);

        let fetched = q.get(&BatchId("batch_cancel".into())).await.unwrap().unwrap();
        assert_eq!(fetched.status, BatchStatus::Cancelled);
    }

    #[tokio::test]
    async fn list_with_pagination() {
        let q = test_queue().await;
        for i in 0..5 {
            let (job, items) = make_job(&format!("batch_list_{i}"));
            q.enqueue(&job, &items).await.unwrap();
        }

        let all = q.list(None, None, 10).await.unwrap();
        assert_eq!(all.len(), 5);

        let page = q.list(None, None, 2).await.unwrap();
        assert_eq!(page.len(), 2);
    }

    #[tokio::test]
    async fn get_items() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_items");
        q.enqueue(&job, &items).await.unwrap();

        let fetched = q.get_items(&BatchId("batch_items".into())).await.unwrap();
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0].custom_id, "req-1");
        assert_eq!(fetched[1].custom_id, "req-2");
    }

    #[tokio::test]
    async fn complete_item_and_batch() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_complete");
        q.enqueue(&job, &items).await.unwrap();

        // Complete both items.
        let result = BatchItemResult {
            status_code: 200,
            body: serde_json::json!({"id": "resp-1"}),
        };
        q.complete_item(&ItemId("batch_complete_item_1".into()), result.clone())
            .await
            .unwrap();
        q.complete_item(&ItemId("batch_complete_item_2".into()), result)
            .await
            .unwrap();

        assert!(q.is_batch_complete(&BatchId("batch_complete".into())).await.unwrap());

        q.complete_batch(&BatchId("batch_complete".into())).await.unwrap();
        let job = q.get(&BatchId("batch_complete".into())).await.unwrap().unwrap();
        assert_eq!(job.status, BatchStatus::Completed);
        assert_eq!(job.request_counts.succeeded, 2);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_batch_engine`
Expected: All queue tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/batch_engine/src/queue/
git commit -m "feat(batch_engine): add JobQueue trait and SqliteQueue implementation"
```

---

## Task 7: Webhook queue and dispatcher (webhook/)

**Files:**
- Create: `crates/batch_engine/src/webhook/mod.rs`
- Create: `crates/batch_engine/src/webhook/sqlite.rs`
- Create: `crates/batch_engine/src/webhook/dispatcher.rs`

- [ ] **Step 1: Write webhook/mod.rs**

```rust
// crates/batch_engine/src/webhook/mod.rs
//! Durable webhook delivery queue and dispatcher.

pub mod dispatcher;
pub mod sqlite;

use crate::error::QueueError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A webhook delivery request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub delivery_id: String,
    pub event_id: String,
    pub batch_id: String,
    pub url: String,
    pub payload: serde_json::Value,
    #[serde(skip)]
    pub signing_secret: Option<String>,
    pub attempts: u8,
    pub max_retries: u8,
    pub next_retry_at: Option<String>,
}

/// A claimed webhook delivery with lease info.
#[derive(Debug)]
pub struct LeasedDelivery {
    pub delivery: WebhookDelivery,
    pub lease_id: String,
}

/// Durable webhook delivery queue.
#[async_trait]
pub trait WebhookQueue: Send + Sync + 'static {
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError>;
    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError>;
    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn schedule_retry(
        &self,
        delivery_id: &str,
        delay: Duration,
    ) -> Result<(), QueueError>;
    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;
}
```

- [ ] **Step 2: Write webhook/sqlite.rs**

```rust
// crates/batch_engine/src/webhook/sqlite.rs
//! SQLite-backed webhook delivery queue.

use super::{LeasedDelivery, WebhookDelivery, WebhookQueue};
use crate::db::now_iso8601;
use crate::error::QueueError;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// SQLite-backed webhook delivery queue.
#[derive(Clone)]
pub struct SqliteWebhookQueue {
    db: Arc<Mutex<Connection>>,
}

impl SqliteWebhookQueue {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl WebhookQueue for SqliteWebhookQueue {
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let payload_str = serde_json::to_string(&delivery.payload)
                .map_err(|e| QueueError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO webhook_delivery
                    (delivery_id, event_id, batch_id, url, payload, signing_secret,
                     status, attempts, max_retries, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9)",
                params![
                    delivery.delivery_id,
                    delivery.event_id,
                    delivery.batch_id,
                    delivery.url,
                    payload_str,
                    delivery.signing_secret,
                    delivery.attempts,
                    delivery.max_retries,
                    now_iso8601(),
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let lease_id = format!("whl_{}", uuid::Uuid::new_v4());
            let now = now_iso8601();
            let lease_expires = {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 60;
                super::super::queue::sqlite::format_epoch_iso8601(secs)
            };

            let result = conn.query_row(
                "UPDATE webhook_delivery
                 SET status = 'processing', lease_id = ?1, lease_expires_at = ?2,
                     attempts = attempts + 1
                 WHERE delivery_id = (
                     SELECT delivery_id FROM webhook_delivery
                     WHERE status = 'pending'
                       AND (next_retry_at IS NULL OR next_retry_at <= ?3)
                     ORDER BY created_at ASC
                     LIMIT 1
                 )
                 RETURNING delivery_id, event_id, batch_id, url, payload,
                           signing_secret, attempts, max_retries, next_retry_at",
                params![lease_id, lease_expires, now],
                |row| {
                    let payload_str: String = row.get(4)?;
                    let payload = serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
                    Ok(LeasedDelivery {
                        delivery: WebhookDelivery {
                            delivery_id: row.get(0)?,
                            event_id: row.get(1)?,
                            batch_id: row.get(2)?,
                            url: row.get(3)?,
                            payload,
                            signing_secret: row.get(5)?,
                            attempts: row.get::<_, i64>(6).unwrap_or(0) as u8,
                            max_retries: row.get::<_, i64>(7).unwrap_or(3) as u8,
                            next_retry_at: row.get(8)?,
                        },
                        lease_id: lease_id.clone(),
                    })
                },
            );

            match result {
                Ok(leased) => Ok(Some(leased)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(QueueError::from(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE webhook_delivery SET status = 'delivered', delivered_at = ?1,
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?2",
                params![now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn schedule_retry(
        &self,
        delivery_id: &str,
        delay: Duration,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
        let retry_at = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + delay.as_secs();
            super::super::queue::sqlite::format_epoch_iso8601(secs)
        };
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE webhook_delivery SET status = 'pending', next_retry_at = ?1,
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?2",
                params![retry_at, id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE webhook_delivery SET status = 'dead_letter',
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?1",
                params![id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let now = now_iso8601();
            let count = conn.execute(
                "UPDATE webhook_delivery SET status = 'pending', lease_id = NULL, lease_expires_at = NULL
                 WHERE lease_id IS NOT NULL AND lease_expires_at < ?1 AND status = 'processing'",
                params![now],
            )?;
            Ok(count as u32)
        })
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_wq() -> SqliteWebhookQueue {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        SqliteWebhookQueue::new(Arc::new(Mutex::new(conn)))
    }

    fn make_delivery(id: &str) -> WebhookDelivery {
        WebhookDelivery {
            delivery_id: id.into(),
            event_id: format!("evt_{id}"),
            batch_id: "batch_1".into(),
            url: "https://example.com/webhook".into(),
            payload: serde_json::json!({"type": "batch.completed"}),
            signing_secret: None,
            attempts: 0,
            max_retries: 3,
            next_retry_at: None,
        }
    }

    #[tokio::test]
    async fn enqueue_and_claim() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_1")).await.unwrap();

        let claimed = wq.claim_next().await.unwrap();
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.delivery.delivery_id, "whd_1");

        // Queue is now empty.
        assert!(wq.claim_next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ack_delivery() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_ack")).await.unwrap();
        let claimed = wq.claim_next().await.unwrap().unwrap();
        wq.ack(&claimed.delivery.delivery_id).await.unwrap();

        // Should not be claimable again.
        assert!(wq.claim_next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn retry_and_dead_letter() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_retry")).await.unwrap();
        let claimed = wq.claim_next().await.unwrap().unwrap();

        // Schedule retry with 0 delay (immediate).
        wq.schedule_retry(&claimed.delivery.delivery_id, Duration::from_secs(0))
            .await
            .unwrap();

        // Should be claimable again.
        let claimed2 = wq.claim_next().await.unwrap();
        assert!(claimed2.is_some());

        // Dead letter.
        wq.dead_letter(&claimed2.unwrap().delivery.delivery_id)
            .await
            .unwrap();
        assert!(wq.claim_next().await.unwrap().is_none());
    }
}
```

- [ ] **Step 3: Write webhook/dispatcher.rs**

```rust
// crates/batch_engine/src/webhook/dispatcher.rs
//! Background webhook delivery loop with HMAC signing and retries.

use super::WebhookQueue;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Configuration for the webhook dispatcher.
pub struct WebhookConfig {
    pub poll_interval: Duration,
    pub reclaim_interval: Duration,
    pub max_concurrent: usize,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            reclaim_interval: Duration::from_secs(30),
            max_concurrent: 8,
        }
    }
}

/// Handle to the running webhook dispatcher.
pub struct WebhookHandle {
    shutdown: CancellationToken,
    join_handle: JoinHandle<()>,
}

impl WebhookHandle {
    pub async fn shutdown(self) {
        self.shutdown.cancel();
        let _ = self.join_handle.await;
    }
}

/// Start the webhook dispatcher background loop.
pub fn start_dispatcher<Q: WebhookQueue>(
    queue: Arc<Q>,
    client: reqwest::Client,
    config: WebhookConfig,
) -> WebhookHandle {
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();

    let join_handle = tokio::spawn(async move {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));
        let mut poll_interval = tokio::time::interval(config.poll_interval);
        let mut reclaim_interval = tokio::time::interval(config.reclaim_interval);

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = reclaim_interval.tick() => {
                    if let Ok(count) = queue.reclaim_expired_leases().await {
                        if count > 0 {
                            tracing::warn!(count, "reclaimed expired webhook leases");
                        }
                    }
                }
                _ = poll_interval.tick() => {
                    let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                        continue;
                    };

                    match queue.claim_next().await {
                        Ok(Some(leased)) => {
                            let queue = queue.clone();
                            let client = client.clone();
                            tokio::spawn(async move {
                                deliver(&queue, &client, &leased.delivery).await;
                                drop(permit);
                            });
                        }
                        Ok(None) => {
                            drop(permit);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "webhook queue claim error");
                            drop(permit);
                        }
                    }
                }
            }
        }
        tracing::info!("webhook dispatcher shut down");
    });

    WebhookHandle {
        shutdown,
        join_handle,
    }
}

async fn deliver<Q: WebhookQueue>(
    queue: &Q,
    client: &reqwest::Client,
    delivery: &super::WebhookDelivery,
) {
    let mut request = client
        .post(&delivery.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Id", &delivery.event_id);

    // HMAC signing.
    if let Some(ref secret) = delivery.signing_secret {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let payload_bytes = serde_json::to_vec(&delivery.payload).unwrap_or_default();
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key length ok");
        mac.update(&payload_bytes);
        let sig = hex::encode(mac.finalize().into_bytes());
        request = request.header("X-Webhook-Signature", format!("sha256={sig}"));
    }

    let response = request.json(&delivery.payload).send().await;

    match response {
        Ok(r) if r.status().is_success() => {
            if let Err(e) = queue.ack(&delivery.delivery_id).await {
                tracing::error!(error = %e, "failed to ack webhook delivery");
            }
        }
        _ => {
            if delivery.attempts < delivery.max_retries {
                let delay = Duration::from_secs(1 << delivery.attempts.min(4));
                if let Err(e) = queue
                    .schedule_retry(&delivery.delivery_id, delay)
                    .await
                {
                    tracing::error!(error = %e, "failed to schedule webhook retry");
                }
            } else {
                tracing::warn!(
                    delivery_id = %delivery.delivery_id,
                    "webhook delivery exhausted retries, moving to dead letter"
                );
                if let Err(e) = queue.dead_letter(&delivery.delivery_id).await {
                    tracing::error!(error = %e, "failed to dead-letter webhook");
                }
            }
        }
    }
}
```

Note: This requires adding `tokio-util` and `hex` to Cargo.toml.

- [ ] **Step 4: Update Cargo.toml with new dependencies**

Add to `crates/batch_engine/Cargo.toml` under `[dependencies]`:

```toml
tokio-util = { version = "0.7", features = ["rt"] }
hex = "0.4"
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p anyllm_batch_engine`
Expected: All tests pass (webhook queue tests + existing tests)

- [ ] **Step 6: Commit**

```bash
git add crates/batch_engine/src/webhook/ crates/batch_engine/Cargo.toml
git commit -m "feat(batch_engine): add durable webhook queue and dispatcher"
```

---

## Task 8: BatchEngine facade (engine.rs)

**Files:**
- Create: `crates/batch_engine/src/engine.rs`

- [ ] **Step 1: Write engine.rs**

```rust
// crates/batch_engine/src/engine.rs
//! BatchEngine: the main entry point for batch operations.
//! Thin facade over JobQueue, FileStore, and WebhookQueue.

use crate::db::now_iso8601;
use crate::error::EngineError;
use crate::file_store::FileStore;
use crate::job::*;
use crate::queue::JobQueue;
use crate::webhook::{WebhookDelivery, WebhookQueue};
use std::sync::Arc;

/// The main batch engine. Holds references to queue, file store, and webhook queue.
pub struct BatchEngine<Q: JobQueue, W: WebhookQueue> {
    pub queue: Arc<Q>,
    pub file_store: FileStore,
    pub webhook_queue: Arc<W>,
    pub global_webhook_urls: Vec<String>,
    pub webhook_signing_secret: Option<String>,
}

impl<Q: JobQueue, W: WebhookQueue> BatchEngine<Q, W> {
    /// Submit a new batch job.
    pub async fn submit(&self, submission: BatchSubmission) -> Result<BatchJob, EngineError> {
        // Verify input file exists.
        self.file_store
            .get_meta(&submission.input_file_id)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?
            .ok_or_else(|| EngineError::FileNotFound(submission.input_file_id.clone()))?;

        let now = now_iso8601();
        let batch_id = BatchId::new();
        let total = submission.items.len() as u32;

        let job = BatchJob {
            id: batch_id.clone(),
            status: BatchStatus::Queued,
            execution_mode: submission.execution_mode.clone(),
            priority: submission.priority,
            key_id: submission.key_id,
            input_file_id: submission.input_file_id,
            webhook_url: submission.webhook_url.clone(),
            metadata: submission.metadata,
            request_counts: RequestCounts {
                total,
                ..Default::default()
            },
            created_at: now.clone(),
            started_at: None,
            completed_at: None,
            expires_at: now.clone(), // TODO: add 24h
        };

        let items: Vec<BatchItem> = submission
            .items
            .into_iter()
            .map(|si| BatchItem {
                id: ItemId::new(),
                batch_id: batch_id.clone(),
                custom_id: si.custom_id,
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: si.model,
                    body: si.body,
                    source_format: si.source_format,
                },
                result: None,
                attempts: 0,
                max_retries: 3,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            })
            .collect();

        self.queue
            .enqueue(&job, &items)
            .await
            .map_err(EngineError::Queue)?;

        // Fire webhook for batch.queued.
        self.fire_webhook(
            &batch_id,
            "batch.queued",
            serde_json::json!({
                "batch_id": batch_id.0,
                "total_items": total,
                "execution_mode": job.execution_mode.as_str(),
            }),
        )
        .await;

        Ok(job)
    }

    /// Get a batch job by ID.
    pub async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, EngineError> {
        self.queue.get(id).await.map_err(EngineError::Queue)
    }

    /// List batch jobs.
    pub async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, EngineError> {
        self.queue
            .list(key_id, cursor, limit)
            .await
            .map_err(EngineError::Queue)
    }

    /// Cancel a batch job.
    pub async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, EngineError> {
        let status = self.queue.cancel(id).await.map_err(EngineError::Queue)?;

        if status == BatchStatus::Cancelled {
            self.fire_webhook(
                id,
                "batch.cancelled",
                serde_json::json!({ "batch_id": id.0 }),
            )
            .await;
        }

        Ok(status)
    }

    /// Get items for a batch (used for result retrieval).
    pub async fn get_items(&self, id: &BatchId) -> Result<Vec<BatchItem>, EngineError> {
        self.queue.get_items(id).await.map_err(EngineError::Queue)
    }

    /// Fire a webhook to all configured URLs.
    async fn fire_webhook(
        &self,
        batch_id: &BatchId,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let event_id = format!("evt_{}", uuid::Uuid::new_v4());

        // Collect URLs: global + per-batch.
        let mut urls: Vec<(String, Option<String>)> = self
            .global_webhook_urls
            .iter()
            .map(|u| (u.clone(), self.webhook_signing_secret.clone()))
            .collect();

        // Per-batch webhook gets terminal events only.
        if matches!(
            event_type,
            "batch.completed" | "batch.failed" | "batch.cancelled"
        ) {
            if let Ok(Some(job)) = self.queue.get(batch_id).await {
                if let Some(url) = job.webhook_url {
                    urls.push((url, self.webhook_signing_secret.clone()));
                }
            }
        }

        let full_payload = serde_json::json!({
            "event_id": event_id,
            "event_type": event_type,
            "data": payload,
        });

        for (url, secret) in urls {
            let delivery = WebhookDelivery {
                delivery_id: format!("whd_{}", uuid::Uuid::new_v4()),
                event_id: event_id.clone(),
                batch_id: batch_id.0.clone(),
                url,
                payload: full_payload.clone(),
                signing_secret: secret,
                attempts: 0,
                max_retries: 3,
                next_retry_at: None,
            };
            if let Err(e) = self.webhook_queue.enqueue(delivery).await {
                tracing::error!(error = %e, "failed to enqueue webhook delivery");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;
    use crate::file_store::FileStore;
    use crate::queue::sqlite::SqliteQueue;
    use crate::webhook::sqlite::SqliteWebhookQueue;
    use rusqlite::Connection;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn test_engine(
    ) -> BatchEngine<SqliteQueue, SqliteWebhookQueue> {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));

        BatchEngine {
            queue: Arc::new(SqliteQueue::new(db.clone())),
            file_store: FileStore::new(db.clone()),
            webhook_queue: Arc::new(SqliteWebhookQueue::new(db)),
            global_webhook_urls: vec![],
            webhook_signing_secret: None,
        }
    }

    #[tokio::test]
    async fn submit_and_get() {
        let engine = test_engine().await;

        // Upload a file first.
        engine
            .file_store
            .insert("file-sub1", None, None, b"test", 2)
            .await
            .unwrap();

        let job = engine
            .submit(BatchSubmission {
                items: vec![
                    SubmissionItem {
                        custom_id: "req-1".into(),
                        model: "gpt-4o".into(),
                        body: serde_json::json!({}),
                        source_format: SourceFormat::OpenAI,
                    },
                    SubmissionItem {
                        custom_id: "req-2".into(),
                        model: "gpt-4o".into(),
                        body: serde_json::json!({}),
                        source_format: SourceFormat::OpenAI,
                    },
                ],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-sub1".into(),
                key_id: Some(42),
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await
            .unwrap();

        assert_eq!(job.status, BatchStatus::Queued);
        assert_eq!(job.request_counts.total, 2);
        assert_eq!(job.key_id, Some(42));

        let fetched = engine.get(&job.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, job.id);
    }

    #[tokio::test]
    async fn submit_missing_file() {
        let engine = test_engine().await;
        let result = engine
            .submit(BatchSubmission {
                items: vec![],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-nope".into(),
                key_id: None,
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancel_job() {
        let engine = test_engine().await;
        engine
            .file_store
            .insert("file-cancel", None, None, b"test", 1)
            .await
            .unwrap();

        let job = engine
            .submit(BatchSubmission {
                items: vec![SubmissionItem {
                    custom_id: "r1".into(),
                    model: "gpt-4o".into(),
                    body: serde_json::json!({}),
                    source_format: SourceFormat::OpenAI,
                }],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-cancel".into(),
                key_id: None,
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await
            .unwrap();

        let status = engine.cancel(&job.id).await.unwrap();
        assert_eq!(status, BatchStatus::Cancelled);
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p anyllm_batch_engine`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/batch_engine/src/engine.rs
git commit -m "feat(batch_engine): add BatchEngine facade with submit, get, list, cancel"
```

---

## Task 9: Make batch_engine compile cleanly

**Files:**
- Modify: `crates/batch_engine/src/lib.rs` (if needed)
- Modify: `crates/batch_engine/src/queue/sqlite.rs` (make `format_epoch_iso8601` pub(crate))
- Modify: `crates/batch_engine/Cargo.toml` (if deps missing)

- [ ] **Step 1: Run full build + clippy**

Run: `cargo build -p anyllm_batch_engine && cargo clippy -p anyllm_batch_engine -- -D warnings`
Expected: Clean build, no warnings

- [ ] **Step 2: Run all tests**

Run: `cargo test -p anyllm_batch_engine`
Expected: All tests pass

- [ ] **Step 3: Commit (if any fixes were needed)**

```bash
git add crates/batch_engine/
git commit -m "fix(batch_engine): fix clippy warnings"
```

---

## Task 10: Wire batch_engine into proxy crate

**Files:**
- Modify: `crates/proxy/Cargo.toml`
- Modify: `crates/proxy/src/server/routes.rs`
- Modify: `crates/proxy/src/batch/mod.rs`
- Modify: `crates/proxy/src/batch/routes.rs`
- Modify: `crates/proxy/src/batch/anthropic_batch.rs`
- Remove: `crates/proxy/src/batch/db.rs` (replaced by batch_engine)
- Remove: `crates/proxy/src/batch/openai_batch_client.rs` (moved to batch_engine, used via proxy-native executor later)
- Modify: `crates/proxy/src/admin/db.rs`

This is the largest task. It refactors the proxy's batch handlers to use `BatchEngine` instead of direct SQLite calls. The existing batch handler logic (translate, call OpenAI, translate back) stays in the proxy for now because native delegation (Phase 2) needs the Executor trait.

**This task is intentionally high-level.** The implementing agent should read each file being modified, understand the current handler logic, and adapt it to use `BatchEngine` methods. The key changes are:

- [ ] **Step 1: Add batch_engine dependency to proxy Cargo.toml**

Add to `crates/proxy/Cargo.toml` under `[dependencies]`:

```toml
anyllm_batch_engine = { path = "../batch_engine", version = "0.2.0" }
```

- [ ] **Step 2: Update AppState to include BatchEngine**

In `crates/proxy/src/server/routes.rs`, add to `AppState`:

```rust
pub batch_engine: Option<Arc<anyllm_batch_engine::BatchEngine<
    anyllm_batch_engine::queue::sqlite::SqliteQueue,
    anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue,
>>>,
```

Set to `None` in test configs that don't need batch, `Some(...)` in server startup.

- [ ] **Step 3: Update batch/mod.rs**

Replace the types and validation in `crates/proxy/src/batch/mod.rs` with re-exports from `batch_engine`. Keep the module declarations for `routes` and `anthropic_batch`. Remove `db` and `openai_batch_client` module declarations.

```rust
// crates/proxy/src/batch/mod.rs
//! Batch API HTTP handlers. Types and logic live in anyllm_batch_engine.

pub mod anthropic_batch;
pub mod routes;

// Re-export engine types for handler use.
pub use anyllm_batch_engine::job::{BatchId, BatchJob, BatchStatus, RequestCounts};
pub use anyllm_batch_engine::validation::{validate_jsonl, ValidatedJsonl};
pub use anyllm_batch_engine::BatchEngine;
```

- [ ] **Step 4: Simplify batch/routes.rs handlers**

Refactor each handler to:
1. Extract `BatchEngine` from `AppState`
2. Call engine methods instead of direct DB operations
3. Return the same OpenAI-compatible JSON responses

Key pattern for each handler:

```rust
async fn create_batch(
    State(state): State<AppState>,
    Extension(vk_ctx): Extension<Option<VirtualKeyContext>>,
    Json(req): Json<CreateBatchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let engine = state.batch_engine.as_ref()
        .ok_or_else(|| bad_request("batch not available"))?;

    // Parse JSONL from stored file, build SubmissionItems...
    // Determine ExecutionMode from backend type...
    // Call engine.submit(...)
    // Format response as OpenAI batch object...
}
```

The `upload_file` handler should use `engine.file_store.insert(...)`.
The `get_batch` handler should use `engine.get(...)`.
The `list_batches` handler should use `engine.list(...)`.

- [ ] **Step 5: Add cancel endpoint**

In `crates/proxy/src/server/routes.rs`, add route:

```rust
.route("/v1/batches/{batch_id}/cancel", post(crate::batch::routes::cancel_batch))
```

In `crates/proxy/src/batch/routes.rs`, add handler:

```rust
pub async fn cancel_batch(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let engine = state.batch_engine.as_ref()
        .ok_or_else(|| bad_request("batch not available"))?;

    let status = engine.cancel(&BatchId(batch_id.clone())).await
        .map_err(|e| internal_error(&e.to_string()))?;

    let job = engine.get(&BatchId(batch_id)).await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found("batch not found"))?;

    Ok(Json(job_to_openai_response(&job)))
}
```

- [ ] **Step 6: Simplify anthropic_batch.rs handlers**

Same pattern: use `BatchEngine` for job lifecycle, keep the translation logic (calling `translate_batch_to_openai_jsonl`, etc.) for the Anthropic-format endpoints. The existing `OpenAIBatchClient` usage stays temporarily for native delegation.

- [ ] **Step 7: Remove old batch/db.rs**

Delete `crates/proxy/src/batch/db.rs`. All its functionality is now in `batch_engine`.

- [ ] **Step 8: Remove batch table creation from admin/db.rs**

In `crates/proxy/src/admin/db.rs`, remove the `init_batch_tables` call and the batch table SQL from `init_db`. Add a call to `anyllm_batch_engine::db::init_batch_engine_tables` instead (or call it during server startup alongside admin DB init).

- [ ] **Step 9: Update server startup to initialize BatchEngine**

In the proxy's main.rs or server startup code, after opening the SQLite connection:

```rust
// Initialize batch engine tables.
anyllm_batch_engine::db::init_batch_engine_tables(&conn)?;
anyllm_batch_engine::db::migrate_old_tables(&conn)?;

let db = Arc::new(Mutex::new(conn));
let batch_engine = Arc::new(BatchEngine {
    queue: Arc::new(SqliteQueue::new(db.clone())),
    file_store: FileStore::new(db.clone()),
    webhook_queue: Arc::new(SqliteWebhookQueue::new(db.clone())),
    global_webhook_urls: parse_webhook_urls(),
    webhook_signing_secret: std::env::var("BATCH_WEBHOOK_SIGNING_SECRET").ok(),
});

// Start webhook dispatcher.
let webhook_handle = anyllm_batch_engine::webhook::dispatcher::start_dispatcher(
    batch_engine.webhook_queue.clone(),
    reqwest::Client::new(),
    WebhookConfig::default(),
);
```

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: All ~906+ tests pass. Batch API integration tests in `crates/proxy/tests/batch_api.rs` should still pass with the new engine wiring.

- [ ] **Step 11: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: Clean

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "refactor(proxy): wire batch handlers through BatchEngine crate

Proxy batch handlers now use BatchEngine for job lifecycle, file storage,
and webhook delivery instead of direct SQLite calls. Old batch/db.rs
removed. Cancel endpoint added at POST /v1/batches/{id}/cancel.
Virtual key attribution (key_id) now populated on batch jobs."
```

---

## Task 11: Update existing batch integration tests

**Files:**
- Modify: `crates/proxy/tests/batch_api.rs`

- [ ] **Step 1: Add cancel test**

```rust
#[tokio::test]
async fn cancel_queued_batch() {
    let (addr, _state) = spawn_test_server_with_shared().await;
    let client = reqwest::Client::new();

    // Upload + create batch.
    let file_id = upload_test_file(&client, &addr).await;
    let batch = create_test_batch(&client, &addr, &file_id).await;
    let batch_id = batch["id"].as_str().unwrap();

    // Cancel it.
    let resp = client
        .post(format!("http://{addr}/v1/batches/{batch_id}/cancel"))
        .header("x-api-key", "test-key")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["status"] == "cancelled" || body["status"] == "cancelling",
        "unexpected status: {}",
        body["status"]
    );
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test batch_api`
Expected: All tests pass including new cancel test

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/tests/batch_api.rs
git commit -m "test: add batch cancellation integration test"
```

---

## Task 12: Final verification

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: Clean

- [ ] **Step 2: Full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Clippy**

Run: `cargo clippy -- -D warnings`
Expected: Clean

- [ ] **Step 4: Format check**

Run: `cargo fmt --check`
Expected: Clean
