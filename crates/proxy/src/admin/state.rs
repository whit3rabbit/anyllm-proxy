// Shared state between the proxy and admin server.
// RuntimeConfig holds mutable settings; AdminEvent is broadcast to WebSocket clients.

use crate::admin::keys::VirtualKeyMeta;
use crate::config::ModelMapping;
use crate::metrics::Metrics;
use dashmap::DashMap;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

/// Type-erased closure that reloads the tracing filter at runtime.
/// Returns true on success, false if the filter string is invalid.
pub type LogReloadFn = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Shared between proxy handlers and the admin server.
#[derive(Clone)]
pub struct SharedState {
    /// SQLite connection for request logging and config persistence.
    /// Uses std::sync::Mutex (not tokio::sync::Mutex) because rusqlite
    /// is synchronous; holding a tokio Mutex guard across .await would
    /// require the guard to be Send, which std::sync satisfies.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    /// Broadcast channel sender for live dashboard updates.
    pub events_tx: broadcast::Sender<AdminEvent>,
    /// Runtime-mutable config read on every proxy request.
    /// std::sync::RwLock (not tokio): proxy reads are synchronous and
    /// frequent; async locking would add unnecessary overhead. Write
    /// contention is negligible since only the admin API writes.
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    /// Per-backend metrics (same Arc the proxy already uses).
    pub backend_metrics: Arc<HashMap<String, Metrics>>,
    /// Write buffer sender for batched SQLite inserts.
    pub log_tx: tokio::sync::mpsc::Sender<RequestLogEntry>,
    /// Closure to reload tracing filter at runtime. None in tests.
    pub log_reload: Option<LogReloadFn>,
    /// Serializes config write operations (Phase 1: SQLite + Phase 2: in-memory)
    /// so concurrent PUT /admin/api/config requests cannot interleave.
    pub config_write_lock: Arc<tokio::sync::Mutex<()>>,
    /// In-memory cache of active virtual API keys, keyed by SHA-256 hash bytes.
    /// Populated from SQLite at startup; updated on create/revoke via admin API.
    pub virtual_keys: Arc<DashMap<[u8; 32], VirtualKeyMeta>>,
}

/// Run a synchronous closure against the SQLite connection on the blocking
/// threadpool. Recovers from mutex poisoning (unwrap_or_else on into_inner)
/// because a panic in one request should not permanently lock out the DB.
/// Returns None if spawn_blocking itself panicked (should not happen).
pub async fn with_db<F, T>(db: &Arc<Mutex<rusqlite::Connection>>, f: F) -> Option<T>
where
    F: FnOnce(&rusqlite::Connection) -> T + Send + 'static,
    T: Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn)
    })
    .await
    .ok()
}

/// Runtime-mutable configuration. Changes via admin UI take effect immediately.
/// Env vars are the defaults; overrides from SQLite take precedence.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Per-backend model mappings (key = backend name).
    pub model_mappings: IndexMap<String, ModelMapping>,
    /// Tracing filter string (e.g., "info", "debug").
    pub log_level: String,
    /// Whether to log request/response bodies at debug level.
    pub log_bodies: bool,
}

/// Events broadcast to WebSocket clients for live dashboard updates.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AdminEvent {
    /// Fired after each proxied request completes.
    #[serde(rename = "request_completed")]
    RequestCompleted(RequestLogEntry),
    /// Periodic metrics summary.
    #[serde(rename = "metrics_snapshot")]
    MetricsSnapshot(MetricsSnapshotData),
    /// Config changed via admin UI.
    #[serde(rename = "config_changed")]
    ConfigChanged { key: String, value: String },
}

/// Data recorded for each proxied request. Stored in SQLite and broadcast
/// to WebSocket clients for the live admin dashboard.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestLogEntry {
    pub request_id: String,
    pub timestamp: String,
    pub backend: String,
    /// Model name from the client's Anthropic request (before mapping).
    pub model_requested: Option<String>,
    /// Model name actually sent to the backend (after mapping).
    pub model_mapped: Option<String>,
    pub status_code: u16,
    pub latency_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Whether the request used SSE streaming. Streaming requests only
    /// track total count in metrics, not per-request success/error.
    pub is_streaming: bool,
    /// Present only when the request failed; contains the error description.
    pub error_message: Option<String>,
}

impl SharedState {
    /// Construct a minimal SharedState for tests (in-memory DB, dummy channel).
    pub fn new_for_test() -> Self {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        crate::admin::db::init_db(&conn).expect("init_db");
        let (events_tx, _) = broadcast::channel(4);
        let (log_tx, _) = tokio::sync::mpsc::channel(4);
        Self {
            db: Arc::new(Mutex::new(conn)),
            events_tx,
            runtime_config: Arc::new(RwLock::new(RuntimeConfig {
                model_mappings: IndexMap::new(),
                log_level: "info".to_string(),
                log_bodies: false,
            })),
            backend_metrics: Arc::new(HashMap::new()),
            log_tx,
            log_reload: None,
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys: Arc::new(DashMap::new()),
        }
    }
}

/// Aggregated metrics for the periodic WebSocket snapshot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshotData {
    pub backends: HashMap<String, crate::metrics::MetricsSnapshot>,
    pub latency_p50_ms: Option<u64>,
    pub latency_p95_ms: Option<u64>,
    pub latency_p99_ms: Option<u64>,
    pub requests_per_second: f64,
    pub error_rate: f64,
}
