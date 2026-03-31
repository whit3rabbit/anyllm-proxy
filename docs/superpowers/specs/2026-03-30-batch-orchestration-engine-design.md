# Batch Orchestration Engine Design

**Date:** 2026-03-30
**Status:** Approved
**Scope:** Unified batch orchestration layer with hybrid execution, job queue, workers, and event-driven notifications

## Problem

The current batch API is a thin pass-through to OpenAI's batch service. It only works with OpenAI and Azure backends, has no background workers (clients must poll), no cancellation, no webhook notifications, and no virtual key attribution. Batch is wired up but not a real product feature.

## Goal

Build a batch orchestration layer that:

1. Presents a single, consistent batch UX regardless of backend
2. Delegates to provider-native batch APIs where available (OpenAI, Azure)
3. Processes batch items locally for backends without native batch support (Gemini, Bedrock, Vertex, Anthropic)
4. Tracks jobs, retries failed items, and notifies consumers via webhooks and SSE

The user experience is: submit work, system owns execution, get results later.

## Architecture Overview

```
Client
  |
  POST /v1/batches (or /v1/messages/batches)
  |
  v
Proxy (axum) ── thin HTTP adapter
  |
  v
BatchEngine (batch_engine crate)
  |
  +-- JobQueue trait (SQLite default, Redis optional)
  |
  +-- WorkerPool (in-process tokio tasks, item-level execution)
  |     |
  |     +-- Executor trait (implemented by proxy crate)
  |     |     +-- execute_item()   -> proxy-native path
  |     |     +-- execute_native() -> provider delegation path
  |     |
  |     +-- Lease renewal, crash recovery, graceful shutdown
  |
  +-- EventBus (topic-based broadcast, lossy, for SSE)
  |
  +-- WebhookQueue (durable SQLite queue, reliable delivery)
  |
  +-- NotificationManager (fan-out: EventBus + WebhookQueue)
```

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Execution model | Hybrid (delegate or execute) | Native batch preserves provider pricing/throughput; proxy-native unlocks all backends |
| Queue infrastructure | SQLite default, Redis optional | Preserves single-binary deployment; Redis scales horizontally when needed |
| Notification | Global webhooks + per-batch webhooks + SSE | Three consumers: ops (global), automation (per-batch), humans (SSE) |
| Worker model | In-process with graceful shutdown | Single binary; designed for future extraction via Executor trait |
| Crate structure | New `batch_engine` crate | Enforced boundary; independently testable; future `anyllm worker` binary |
| Work unit | Item, not job | Items are the execution unit; jobs are lifecycle/aggregation |

## Section 1: Crate Structure

```
crates/
  batch_engine/         # NEW
    src/
      lib.rs
      job.rs            # BatchJob, BatchItem, BatchId, types
      queue/
        mod.rs          # JobQueue trait
        sqlite.rs       # SqliteQueue
        redis.rs        # RedisQueue (behind --features redis-queue)
      worker/
        mod.rs          # WorkerPool, WorkerHandle, WorkerConfig
        executor.rs     # Executor trait, ExecutorError
      event/
        mod.rs          # BatchEvent, BatchEventPayload
        bus.rs          # EventBus trait, TopicEventBus
      notification/
        mod.rs          # NotificationManager, dispatch()
        webhook.rs      # WebhookQueue trait, WebhookDelivery, WebhookDispatcher
        sse.rs          # SSEBroadcaster, BatchSubscriber
      scheduler.rs      # Priority, concurrency control
      lease.rs          # Lease renewal, expiry
      db.rs             # Schema initialization, migrations
  translator/           # UNCHANGED
  client/               # UNCHANGED
  proxy/                # SLIMMED DOWN
    src/batch/          # Thin HTTP handlers only
```

**Dependency graph:**

- `batch_engine` depends on `translator` (for request translation in proxy-native mode)
- `batch_engine` does NOT depend on `proxy`, `axum`, or any HTTP types
- `proxy` depends on `batch_engine`
- Redis queue behind `--features redis-queue`

## Section 2: Core Types

```rust
pub struct BatchId(pub String);   // "batch_{uuid}"
pub struct ItemId(pub String);    // "item_{uuid}"

pub struct BatchJob {
    pub id: BatchId,
    pub status: BatchStatus,
    pub execution_mode: ExecutionMode,
    pub priority: u8,                // 0=lowest, 255=highest
    pub key_id: Option<i64>,         // virtual key attribution
    pub webhook_url: Option<Url>,    // per-batch notification
    pub metadata: Option<serde_json::Value>,
    pub request_counts: RequestCounts,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

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
    /// Delegate to provider's native batch API (OpenAI, Azure)
    Native { provider: String },
    /// Proxy processes items individually
    ProxyNative,
}

pub struct BatchItem {
    pub id: ItemId,
    pub custom_id: String,
    pub status: ItemStatus,
    pub request: BatchItemRequest,
    pub result: Option<BatchItemResult>,
    pub attempts: u8,
    pub max_retries: u8,
    pub last_error: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub lease_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub idempotency_key: Option<String>,
}

pub enum ItemStatus {
    Pending,
    Processing,
    Succeeded,
    Failed,
    Cancelled,
}

pub struct BatchItemRequest {
    pub model: String,
    pub body: serde_json::Value,     // backend-format request
    pub source_format: SourceFormat,
}

pub enum SourceFormat { Anthropic, OpenAI }

pub struct BatchItemResult {
    pub status_code: u16,
    pub body: serde_json::Value,     // response in source_format
}

pub struct RequestCounts {
    pub total: u32,
    pub processing: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub expired: u32,
}
```

**Design notes:**

- `BatchItemRequest.body` is `serde_json::Value`. The engine routes, not interprets. Translation happens at the proxy boundary.
- `ExecutionMode` decided at job creation by the proxy based on backend type.
- `key_id` always populated when virtual keys are in use (fixes current gap).
- `idempotency_key` prevents duplicate processing on retries.

## Section 3: JobQueue Trait

```rust
#[async_trait]
pub trait JobQueue: Send + Sync + 'static {
    // -- Job lifecycle --
    async fn enqueue(&self, job: &BatchJob) -> Result<(), QueueError>;
    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError>;
    async fn list(&self, cursor: Option<&BatchId>, limit: u32) -> Result<Vec<BatchJob>, QueueError>;
    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, QueueError>;

    // -- Item-level operations (proxy-native path) --
    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError>;
    async fn complete_item(&self, id: &ItemId, result: BatchItemResult) -> Result<(), QueueError>;
    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError>;
    async fn schedule_retry(&self, id: &ItemId, delay: Duration, error: &str) -> Result<(), QueueError>;
    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError>;

    // -- Batch completion --
    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError>;
    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError>;

    // -- Native batch support --
    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError>;

    // -- Lease management --
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;

    // -- Progress --
    async fn update_progress(&self, id: &BatchId, counts: &RequestCounts) -> Result<(), QueueError>;
}

pub struct LeasedItem {
    pub item: BatchItem,
    pub batch_id: BatchId,
    pub lease_id: String,
    pub lease_expires_at: DateTime<Utc>,
}

pub enum QueueError {
    NotFound,
    AlreadyClaimed,
    Storage(String),
}
```

**SQLite implementation:**

- `claim_next_item` uses atomic `UPDATE ... WHERE item_id = (SELECT ... LIMIT 1) RETURNING *` to avoid SELECT-then-UPDATE races
- Joins `batch_job` to order by priority, filters by `execution_mode = 'proxy_native'`
- Skips items where `next_retry_at` is in the future
- Also transitions parent job from `queued` to `processing` on first item claim
- Indexes: `(status, next_retry_at, created_at)` for claim, `(batch_id, status)` for completion check, `(lease_expires_at)` for reclaim

**Redis implementation (future, behind feature flag):**

- Sorted set keyed by priority + timestamp for ordering
- `BRPOPLPUSH` for blocking dequeue
- Lease tracking via key expiry
- Job/item metadata still in SQLite (Redis is queue only)

## Section 4: Worker Pool and Executor

```rust
pub struct WorkerConfig {
    pub max_concurrent_items: usize,       // semaphore-bounded, default: 16
    pub lease_duration: Duration,           // item lease, default: 120s
    pub lease_renewal_interval: Duration,   // default: 30s
    pub reclaim_interval: Duration,         // default: 30s
    pub max_item_retries: u8,              // default: 3
    pub retry_base_delay: Duration,         // default: 5s
    pub shutdown_timeout: Duration,         // default: 60s
}

pub struct WorkerPool<Q: JobQueue, E: Executor> {
    queue: Arc<Q>,
    executor: Arc<E>,
    event_bus: Arc<dyn EventBus>,
    config: WorkerConfig,
    shutdown: CancellationToken,
}

impl<Q: JobQueue, E: Executor> WorkerPool<Q, E> {
    pub fn start(self) -> WorkerHandle;
}

pub struct WorkerHandle {
    shutdown: CancellationToken,
    join_handle: JoinHandle<()>,
}

impl WorkerHandle {
    /// Signal shutdown: stop dequeuing, finish in-flight, requeue rest.
    pub async fn shutdown(self) -> Result<(), EngineError>;
}
```

**Executor trait (implemented in proxy crate, not batch_engine):**

```rust
#[async_trait]
pub trait Executor: Send + Sync + 'static {
    /// Process a single item (proxy-native path).
    async fn execute_item(&self, item: &BatchItem) -> Result<BatchItemResult, ExecutorError>;

    /// Delegate entire batch to provider (native path).
    async fn execute_native(&self, job: &BatchJob) -> Result<NativeBatchResult, ExecutorError>;
}

pub struct NativeBatchResult {
    pub provider_batch_id: String,
    pub items: Vec<(String, BatchItemResult)>,
}

pub enum ExecutorError {
    Retryable(String),    // rate limit, timeout, 5xx
    Fatal(String),        // bad request, auth failure
}
```

**Two separate worker loops:**

Loop 1 -- Item workers (proxy-native batches):

```
loop {
    select! {
        _ = shutdown.cancelled() => break,
        _ = reclaim_timer.tick() => queue.reclaim_expired_leases(),
        permit = item_semaphore.acquire() => {
            match queue.claim_next_item().await {
                Some(leased) => spawn process_item(leased, permit),
                None => sleep(poll_interval),
            }
        }
    }
}
```

Loop 2 -- Native batch poller (delegated batches):

```
loop {
    select! {
        _ = shutdown.cancelled() => break,
        _ = native_poll_timer.tick() => {
            for job in queue.get_native_jobs_in_progress() {
                spawn poll_native_job(job)
            }
        }
    }
}

async fn poll_native_job(job: BatchJob) {
    let status = executor.poll_native(&job).await;
    queue.update_progress(job.id, status.counts);

    // Emit Started on first poll that shows progress
    if job.started_at.is_none() && status.counts.processing > 0 {
        notification.dispatch(Started { .. }).await;
    }

    // Emit Progress on every poll with updated counts
    notification.dispatch(Progress { counts: status.counts }).await;

    if status.is_terminal() {
        let results = executor.fetch_native_results(&job).await;
        // store results, complete batch
        notification.dispatch(Completed { .. }).await;
    }
}
```

**process_item:**

```
async fn process_item(leased: LeasedItem) {
    spawn item_lease_renewal(leased.item_id, leased.lease_id)

    match executor.execute_item(&leased.item).await {
        Ok(result) => {
            queue.complete_item(item_id, result)
            notification.dispatch(ItemCompleted { .. }).await
        }
        Err(Retryable(msg)) if item.attempts < max_retries => {
            let delay = base_delay * 2^attempts  // exponential backoff
            queue.schedule_retry(item_id, delay, msg)
        }
        Err(e) => {
            queue.fail_item(item_id, e)
            queue.dead_letter(item_id)
            notification.dispatch(ItemFailed { .. }).await
        }
    }

    if queue.is_batch_complete(batch_id) {
        queue.complete_batch(batch_id)
        notification.dispatch(BatchCompleted { .. }).await
    }

    cancel item_lease_renewal
    drop semaphore permit
}
```

**Graceful shutdown sequence:**

1. Cancel the `CancellationToken`
2. Stop dequeuing new items
3. Wait for in-flight items to complete (up to `shutdown_timeout`)
4. Requeue any items still processing after timeout
5. Exit cleanly

**Future extraction path:** The `Executor` trait boundary means a future `anyllm worker` binary simply instantiates `WorkerPool` with the same `Executor` implementation and connects to the shared queue (Redis).

## Section 5: Event Bus and Notifications

### Event Model

```rust
pub struct BatchEvent {
    pub event_id: String,           // "evt_{uuid}" -- idempotency key
    pub sequence: u64,              // monotonic per batch_id
    pub timestamp: DateTime<Utc>,
    pub batch_id: BatchId,
    pub payload: BatchEventPayload,
}

pub enum BatchEventPayload {
    Queued { total_items: u32, execution_mode: ExecutionMode, key_id: Option<i64> },
    Started,
    Progress { counts: RequestCounts },
    ItemCompleted { item_id: ItemId, custom_id: String, succeeded: bool },
    Completed { counts: RequestCounts, duration_secs: u64 },
    Failed { error: String, counts: RequestCounts },
    Cancelled { counts: RequestCounts },
}
```

### Two-Path Fan-Out

```
Worker emits event
    +-- EventBus (broadcast, lossy) --> SSE subscribers (real-time, best-effort)
    +-- WebhookQueue (durable, SQLite) --> WebhookDispatcher (reliable, retried)
```

The engine never sends events through a single path. Every event goes to both. All event emission MUST go through `NotificationManager::dispatch()`. Workers must not call `event_bus.emit()` directly.

### EventBus (SSE path, lossy)

```rust
pub trait EventBus: Send + Sync + 'static {
    fn emit(&self, event: BatchEvent);
    fn subscribe_batch(&self, id: &BatchId) -> BatchSubscriber;
}
```

**Implementation: `TopicEventBus`** using per-batch `tokio::sync::broadcast` channels (capacity 256). Topics created on first subscriber or emit. Cleaned up when batch reaches terminal state.

```rust
pub struct TopicEventBus {
    topics: DashMap<BatchId, broadcast::Sender<BatchEvent>>,
    global: broadcast::Sender<BatchEvent>,
}
```

No replay. Reconnecting clients get current state via `GET /v1/batches/{id}` then resubscribe.

### WebhookQueue (durable path)

```rust
pub trait WebhookQueue: Send + Sync + 'static {
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError>;
    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError>;
    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn schedule_retry(&self, delivery_id: &str, delay: Duration) -> Result<(), QueueError>;
    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;
}

pub struct WebhookDelivery {
    pub delivery_id: String,        // "whd_{uuid}"
    pub event_id: String,           // receiver-side dedup key
    pub url: Url,
    pub payload: serde_json::Value,
    pub signing_secret: Option<String>,
    pub attempts: u8,
    pub max_retries: u8,            // default: 3
    pub next_retry_at: Option<DateTime<Utc>>,
}
```

### Dispatch (single call site)

```rust
pub async fn dispatch(&self, event: BatchEvent) {
    self.event_bus.emit(event.clone());           // SSE (lossy)
    for sink in self.resolve_sinks(&event) {
        self.webhook_queue.enqueue(WebhookDelivery {
            delivery_id: format!("whd_{}", Uuid::new_v4()),
            event_id: event.event_id.clone(),
            url: sink.url,
            payload: serde_json::to_value(&event).unwrap(),
            signing_secret: sink.signing_secret,
            attempts: 0,
            max_retries: 3,
            next_retry_at: None,
        });
    }
}
```

Global webhooks receive all events. Per-batch webhooks receive terminal events only (`Completed`, `Failed`, `Cancelled`).

### WebhookDispatcher (background loop)

Retries with exponential backoff (1s, 2s, 4s). Dead-letters after max retries. Runs a lease reclaim loop (same pattern as item queue) to recover stuck deliveries from crashed dispatchers.

**Webhook HTTP headers:**

| Header | Value |
|--------|-------|
| `X-Webhook-Id` | `event_id` (receiver dedup key) |
| `X-Webhook-Signature` | `sha256={hmac}` (when signing secret configured) |
| `Content-Type` | `application/json` |

### Event Persistence (optional)

Events stored in `batch_event_log` table with 7-day TTL. Used for debugging and webhook replay. Not required for SSE.

## Section 6: Proxy Integration

The proxy crate becomes a thin HTTP adapter. Existing handler logic moves into the engine; handlers shrink to request parsing, engine calls, and response formatting.

### Engine Initialization (server startup)

```rust
let db = open_sqlite(db_path);
let queue = Arc::new(SqliteQueue::new(db.clone()));
let event_bus = Arc::new(TopicEventBus::new());
let webhook_queue = Arc::new(SqliteWebhookQueue::new(db.clone()));

let executor = Arc::new(ProxyExecutor::new(backend_client.clone()));

let engine = BatchEngine {
    queue: queue.clone(),
    event_bus: event_bus.clone(),
    webhook_queue: webhook_queue.clone(),
};

let worker_handle = WorkerPool::new(
    queue.clone(), executor, event_bus.clone(), WorkerConfig::from_env(),
).start();

let webhook_handle = WebhookDispatcher::new(
    webhook_queue, WebhookConfig::from_env(),
).start();

// Graceful shutdown
tokio::signal::ctrl_c().await;
worker_handle.shutdown().await;
webhook_handle.shutdown().await;
```

### Routes

```
// OpenAI-compatible
POST /v1/files                       -> upload JSONL, validate, store
POST /v1/batches                     -> parse, determine ExecutionMode, engine.submit()
GET  /v1/batches/:id                 -> engine.queue.get(), serialize OpenAI format
GET  /v1/batches                     -> engine.queue.list(), paginate
POST /v1/batches/:id/cancel          -> engine.queue.cancel()           [NEW]

// Anthropic-compatible (translate mode only)
POST /v1/messages/batches            -> translate, engine.submit()
GET  /v1/messages/batches/:id        -> engine.queue.get(), translate to Anthropic format
GET  /v1/messages/batches/:id/results -> engine.get_results(), translate each line
POST /v1/messages/batches/:id/cancel  -> engine.queue.cancel()          [NEW]

// SSE stream [NEW]
GET  /v1/batches/:id/stream          -> subscribe, map events to SSE frames
```

### ExecutionMode Decision

```rust
let mode = match &backend {
    BackendClient::OpenAI(_) | BackendClient::AzureOpenAI(_) => {
        ExecutionMode::Native { provider: backend.name().into() }
    }
    _ => ExecutionMode::ProxyNative,
};
```

### ProxyExecutor (lives in proxy crate)

Implements `batch_engine::Executor`. Bridges the engine to the proxy's backend clients.

```rust
impl Executor for ProxyExecutor {
    async fn execute_item(&self, item: &BatchItem) -> Result<BatchItemResult, ExecutorError> {
        // Route through existing backend client
        // Map BackendError to ExecutorError (Retryable vs Fatal)
    }

    async fn execute_native(&self, job: &BatchJob) -> Result<NativeBatchResult, ExecutorError> {
        // Refactored from current OpenAIBatchClient logic
        // Upload JSONL, create batch, poll until complete
    }
}
```

### SSE Handler

```rust
async fn sse_stream(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let id = BatchId(batch_id);
    let job = state.batch_engine.queue.get(&id).await?.ok_or(ApiError::NotFound)?;

    let mut subscriber = state.sse_broadcaster.subscribe(&id);
    let stream = async_stream::stream! {
        // Emit snapshot for late subscribers so they see current state immediately
        let snapshot = BatchEvent {
            event_id: format!("evt_snapshot_{}", Uuid::new_v4()),
            sequence: 0,
            timestamp: Utc::now(),
            batch_id: id.clone(),
            payload: BatchEventPayload::Progress { counts: job.request_counts.clone() },
        };
        yield Ok(Event::default().event("batch.snapshot").data(
            serde_json::to_string(&snapshot).unwrap()
        ));

        while let Some(event) = subscriber.next().await {
            let event_type = event.payload.event_type();
            let data = serde_json::to_string(&event).unwrap();
            yield Ok(Event::default().event(event_type).data(data));
            if event.payload.is_terminal() { break; }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
```

### What Changes in proxy/src/batch/

- `openai_batch_client.rs` logic moves into `ProxyExecutor::execute_native`
- `anthropic_batch.rs` handlers become thin (translate, submit, translate response)
- `routes.rs` handlers shrink to parsing + engine calls + formatting
- `db.rs` batch tables move to `batch_engine`; proxy keeps admin-only tables

### New Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `BATCH_MAX_CONCURRENT_ITEMS` | `16` | Item-level semaphore |
| `BATCH_LEASE_DURATION_SECS` | `120` | Item lease timeout |
| `BATCH_MAX_ITEM_RETRIES` | `3` | Per-item retry limit |
| `BATCH_RETRY_BASE_DELAY_SECS` | `5` | Exponential backoff base |
| `BATCH_WEBHOOK_SIGNING_SECRET` | none | HMAC key for webhook signatures |
| `BATCH_EVENT_RETENTION_DAYS` | `7` | Event log TTL |

## Section 7: SQLite Schema

```sql
-- Job queue
CREATE TABLE IF NOT EXISTS batch_job (
    batch_id          TEXT PRIMARY KEY,
    status            TEXT NOT NULL DEFAULT 'queued',
    execution_mode    TEXT NOT NULL,
    provider          TEXT,
    provider_batch_id TEXT,
    priority          INTEGER NOT NULL DEFAULT 0,
    key_id            INTEGER,
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

-- Items (unit of work)
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

-- Dead letter
CREATE TABLE IF NOT EXISTS batch_dead_letter (
    item_id      TEXT PRIMARY KEY,
    batch_id     TEXT NOT NULL,
    custom_id    TEXT NOT NULL,
    request_body TEXT NOT NULL,
    last_error   TEXT,
    attempts     INTEGER NOT NULL,
    failed_at    TEXT NOT NULL
);

-- Batch files (uploaded JSONL)
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

-- Anthropic batch mapping
CREATE TABLE IF NOT EXISTS anthropic_batch_map (
    our_batch_id    TEXT PRIMARY KEY,
    engine_batch_id TEXT NOT NULL,
    model           TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

-- Webhook delivery queue (durable)
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

-- Event log (debugging + webhook replay)
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
```

**Key query: claim_next_item (atomic dequeue):**

```sql
UPDATE batch_item
SET status = 'processing',
    lease_id = ?1,
    lease_expires_at = ?2,
    attempts = attempts + 1
WHERE item_id = (
    SELECT bi.item_id
    FROM batch_item bi
    JOIN batch_job bj ON bi.batch_id = bj.batch_id
    WHERE bi.status = 'pending'
      AND (bi.next_retry_at IS NULL OR bi.next_retry_at <= ?3)
      AND bj.status IN ('queued', 'processing')
      AND bj.execution_mode = 'proxy_native'
    ORDER BY bj.priority DESC, bi.created_at ASC
    LIMIT 1
)
RETURNING *;
```

**Migration:** Old `batch_job` and `batch_file` tables renamed to `_v1` suffix. `anthropic_batch_map.openai_batch_id` column renamed to `engine_batch_id`.

## Implementation Phases

### Phase 1: Enhanced pass-through (low effort, high impact)

- Create `batch_engine` crate with core types and `JobQueue` trait
- Implement `SqliteQueue`
- Refactor existing batch handlers to use the engine
- Add batch cancellation endpoint
- Add virtual key attribution (`key_id`)
- Add per-batch webhook URL
- Wire global webhook delivery (durable queue)

### Phase 2: Proxy-native batch (differentiator)

- Implement `WorkerPool` with item-level execution
- Implement `Executor` trait in proxy crate (`ProxyExecutor`)
- Add graceful shutdown with lease management
- Add exponential backoff retries + dead letter
- Unlock batch for Gemini, Bedrock, Vertex, Anthropic backends

### Phase 3: Event system and SSE

- Implement `TopicEventBus`
- Add `GET /v1/batches/{id}/stream` SSE endpoint
- Add event persistence (batch_event_log)
- Wire event dispatch into worker loop

### Phase 4: Optimization

- Add Redis queue backend (behind feature flag)
- Hybrid execution mode selection (auto-detect best path)
- Priority scheduling
- Concurrency tuning

## Scale Targets

| Mode | Concurrent jobs | Instances | Workers |
|------|----------------|-----------|---------|
| SQLite (default) | 10-200 | 1 | 4-16 in-process |
| Redis (optional) | 200-10,000+ | N | horizontal |

## Resource Isolation

Batch workers must not starve interactive API requests:

- Item-level semaphore (`BATCH_MAX_CONCURRENT_ITEMS`) caps LLM calls from batch
- Interactive requests always take priority (no semaphore)
- Batch respects per-key rate limits (RPM/TPM)
- Backpressure: if queue depth exceeds threshold, reject new batch submissions with 429
