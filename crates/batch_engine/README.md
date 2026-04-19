# anyllm_batch_engine

Batch orchestration engine: job queue, file storage, worker pool, and webhook delivery. HTTP-agnostic. Part of the [anyllm-proxy](https://github.com/whit3rabbit/anyllm-proxy) workspace.

## What this crate is

The backend of an OpenAI-style Batch API, packaged as a library. Callers (typically `anyllm_proxy`) wire its public types into their own HTTP layer; this crate owns the persistence, the worker loop, and the webhook signing.

It owns:

- A SQLite-backed job queue (`rusqlite`, bundled, no external server).
- A file store for input and output JSONL.
- An async worker pool that pulls queued jobs and forwards them to a translator-compatible backend.
- HMAC-signed webhook delivery for completion notifications.
- JSONL request validation (`validate_jsonl`).

It deliberately does **not** own:

- HTTP handlers or routes. The proxy crate exposes the OpenAI-compatible REST surface.
- Authentication or rate limiting. Those belong to the surrounding server.

## Where it fits

Five-crate workspace:

- `anyllm_translate` - pure format mapping, no I/O.
- `anyllm_providers` - provider and model catalog.
- `anyllm_client` - async HTTP client.
- `anyllm_batch_engine` (this crate) - batch job queue and worker pool.
- `anyllm_proxy` - axum HTTP server that wires this crate into the `/v1/batches` and `/v1/files` endpoints.

Depend on this crate directly if you are building your own batch API surface. Depend on `anyllm_proxy` if you want the HTTP endpoints and admin UI for free.

## Add it

```toml
[dependencies]
anyllm_batch_engine = "0.9"
```

## Library examples

### 1. Validate a JSONL submission before accepting it

`validate_jsonl` is pure and synchronous. It enforces the OpenAI batch contract: every line is JSON, every `custom_id` is unique and ≤64 chars, every `body` is an object with a `model` field, ≤50,000 lines, ≤100 MB. Use it in any HTTP handler or CLI before persisting the file.

```rust
use anyllm_batch_engine::validate_jsonl;
use std::io::Cursor;

let input = br#"{"custom_id":"a","method":"POST","url":"/v1/chat/completions","body":{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}}
{"custom_id":"b","method":"POST","url":"/v1/chat/completions","body":{"model":"gpt-4o-mini","messages":[{"role":"user","content":"there"}]}}
"#;

let result = validate_jsonl(Cursor::new(&input[..]))
    .map_err(|e| format!("rejected: {e}"))?;
println!("accepted {} items", result.line_count);
```

### 2. Wire up an in-memory engine for tests

The crate ships SQLite-backed `JobQueue` and `WebhookQueue` implementations. Both can share an in-memory connection, which is what the crate's own tests use; this is the smallest end-to-end example.

```rust
use anyllm_batch_engine::{
    BatchEngine, BatchSubmission, ExecutionMode, SourceFormat, SubmissionItem,
};
use anyllm_batch_engine::db::init_batch_engine_tables;
use anyllm_batch_engine::file_store::FileStore;
use anyllm_batch_engine::queue::sqlite::SqliteQueue;
use anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

let conn = Connection::open_in_memory()?;
init_batch_engine_tables(&conn)?;
let db = Arc::new(Mutex::new(conn));

let engine = BatchEngine {
    queue: Arc::new(SqliteQueue::new(db.clone())),
    file_store: FileStore::new(db.clone()),
    webhook_queue: Arc::new(SqliteWebhookQueue::new(db)),
    global_webhook_urls: vec![],
    webhook_signing_secret: None,
};

// Upload a (here, trivial) JSONL file the submission will reference.
engine.file_store
    .insert("file-demo", None, None, b"{\"custom_id\":\"a\",\"body\":{\"model\":\"gpt-4o-mini\"}}", 1)
    .await?;

let job = engine.submit(BatchSubmission {
    items: vec![SubmissionItem {
        custom_id: "a".into(),
        model: "gpt-4o-mini".into(),
        body: serde_json::json!({"messages":[{"role":"user","content":"hi"}]}),
        source_format: SourceFormat::OpenAI,
    }],
    execution_mode: ExecutionMode::ProxyNative,
    input_file_id: "file-demo".into(),
    key_id: None,
    webhook_url: None,
    metadata: None,
    priority: 0,
}).await?;

println!("queued {} with {} items", job.id, job.request_counts.total);
```

### 3. Persistent engine + global webhooks

For a long-running process, point the connection at a file and pre-populate `global_webhook_urls` so every job lifecycle event (`batch.queued`, `batch.cancelled`, ...) gets delivered with HMAC-SHA256 signing.

```rust
use anyllm_batch_engine::BatchEngine;
use anyllm_batch_engine::db::init_batch_engine_tables;
use anyllm_batch_engine::file_store::FileStore;
use anyllm_batch_engine::queue::sqlite::SqliteQueue;
use anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

let conn = Connection::open("/var/lib/myapp/batches.sqlite")?;
init_batch_engine_tables(&conn)?;
let db = Arc::new(Mutex::new(conn));

let engine = BatchEngine {
    queue: Arc::new(SqliteQueue::new(db.clone())),
    file_store: FileStore::new(db.clone()),
    webhook_queue: Arc::new(SqliteWebhookQueue::new(db)),
    global_webhook_urls: vec!["https://hooks.example.com/batch".into()],
    webhook_signing_secret: Some(std::env::var("WEBHOOK_SIGNING_SECRET")?),
};
```

Webhook receivers verify deliveries with the `X-Signature-256` header.

### 4. Polling and result retrieval

Once jobs are queued, `get`, `list`, and `get_items` are how you build a status page or pull results.

```rust
use anyllm_batch_engine::{BatchId, BatchStatus};

let job = engine.get(&BatchId("batch_...".into())).await?;
if let Some(job) = job {
    if job.status == BatchStatus::Completed {
        for item in engine.get_items(&job.id).await? {
            if let Some(result) = item.result {
                println!("{} -> {} {}", item.custom_id, result.status_code, result.body);
            }
        }
    }
}

// Paginated listing for an admin UI:
let recent = engine.list(None, None, 50).await?;
```

### 5. Custom queue or webhook backends

`BatchEngine<Q, W>` is generic over the `JobQueue` and `WebhookQueue` traits. Implement either trait against Postgres, Redis, or your own store and plug it in without touching engine code. The bundled `SqliteQueue` / `SqliteWebhookQueue` are the reference implementations.

## Public surface

Top-level re-exports (see `src/lib.rs`):

- `BatchEngine` - the orchestrator (generic over queue and webhook implementations).
- `EngineError`, `QueueError` - error types.
- Types from `job` - `BatchJob`, `BatchStatus`, `BatchItem`, `BatchSubmission`, `SubmissionItem`, `ExecutionMode`, `SourceFormat`, `BatchId`, `ItemId`, `RequestCounts`.
- `validate_jsonl`, `ValidatedJsonl` - input validation for OpenAI-style batch requests.

Modules:

| Module | Purpose |
|---|---|
| `engine` | `BatchEngine` lifecycle: spawn workers, submit jobs, query status. |
| `queue` | SQLite-backed FIFO with claim/ack semantics. |
| `db` | Schema and rusqlite helpers. |
| `file_store` | Input/output JSONL on disk, content-addressed. |
| `job` | Job, status, and input/output types. |
| `validation` | JSONL request validation. |
| `webhook` | HMAC-signed completion callbacks. |
| `error` | `EngineError` and `QueueError`. |

## Tests

```bash
cargo test -p anyllm_batch_engine
```
