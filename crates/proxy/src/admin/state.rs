//! Shared state between the proxy and admin server.
//!
//! `SharedState` is cloned into every axum handler (cheap: all fields are `Arc`).
//! `RuntimeConfig` holds settings that can be mutated at runtime via the admin API
//! without restarting the process. `AdminEvent` is broadcast to WebSocket clients
//! for live dashboard updates.

use crate::admin::keys::VirtualKeyMeta;
use crate::backend::BackendClient;
use crate::config::ModelMapping;
use crate::metrics::Metrics;
use anyllm_providers::ProviderCatalog;
use dashmap::DashMap;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
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
    /// Immutable runtime config defaults loaded from env/config files before DB overrides.
    pub runtime_defaults: RuntimeConfigDefaults,
    /// Per-backend metrics (same Arc the proxy already uses).
    pub backend_metrics: Arc<HashMap<String, Metrics>>,
    /// Write buffer sender for batched SQLite inserts.
    pub log_tx: tokio::sync::mpsc::Sender<RequestLogEntry>,
    /// Closure to reload tracing filter at runtime. None in tests.
    pub log_reload: Option<LogReloadFn>,
    /// Serializes config write operations (Phase 1: SQLite + Phase 2: in-memory)
    /// so concurrent PUT /admin/api/config requests cannot interleave.
    pub config_write_lock: Arc<tokio::sync::Mutex<()>>,
    /// In-memory cache of active virtual API keys, keyed by hash bytes
    /// (HMAC-SHA256 for new keys, legacy SHA-256 for pre-HMAC keys).
    /// Populated from SQLite at startup; updated on create/revoke via admin API.
    pub virtual_keys: Arc<DashMap<[u8; 32], VirtualKeyMeta>>,
    /// Per-installation HMAC secret for keyed hashing of virtual API keys.
    /// Generated once and persisted in the settings table.
    pub hmac_secret: Arc<Vec<u8>>,
    /// Model router for dynamic model management. None unless LiteLLM config is active.
    pub model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    /// Immutable provider/model catalog used by the admin UI and proxy runtime.
    pub provider_catalog: Arc<ProviderCatalog>,
    /// MCP server manager for tool discovery and execution. None when tool execution is disabled.
    pub mcp_manager: Option<Arc<crate::tools::McpServerManager>>,
    /// In-memory set of CSRF tokens issued by GET /admin/csrf-token.
    /// Tokens are removed via `invalidate()` on first successful CSRF validation
    /// (one-time use). moka Cache enforces a hard cap of 1,000 entries and a
    /// 24-hour TTL, preventing unbounded growth from unauthenticated callers.
    pub issued_csrf_tokens: Arc<moka::sync::Cache<String, ()>>,
    /// Unix timestamp of admin server startup; used by /admin/api/uptime.
    pub started_at: std::time::SystemTime,
    /// In-memory registry of managed backends loaded from SQLite at startup.
    /// Key = backend name (same as `row.name`). Keyed by name so routing lookups
    /// can find by backend_name string. Value = (row snapshot, live BackendClient).
    /// Wrapped in RwLock so the admin CRUD routes can update it without restart.
    pub managed_backends:
        Arc<RwLock<HashMap<String, (crate::admin::db::ManagedBackendRow, BackendClient)>>>,
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
    /// Whether to redact detected secrets from upstream JSON/text request payloads.
    pub redact_secrets: bool,
    /// Whether Anthropic thinking-block record-and-restore repair is active
    /// (BACKEND=anthropic passthrough only; see `crate::thinking_repair`).
    pub anthropic_thinking_repair: bool,
    /// Opt-in tool-call guardrail preset, stored as the stable string form of
    /// `crate::tools::ToolGuardrailMode` (see `ToolGuardrailMode::as_str`),
    /// e.g. "disabled" or "standard". Runtime-tunable like the other fields
    /// above; the startup-time `ToolEngineState.guardrails` config is a
    /// separate, static value built from YAML/env at process start.
    pub tool_guardrail_mode: String,
}

/// Runtime config defaults before SQLite overrides are applied. Used to restore
/// the effective runtime value when an override is deleted.
#[derive(Debug, Clone)]
pub struct RuntimeConfigDefaults {
    pub log_bodies: bool,
    pub redact_secrets: bool,
    pub anthropic_thinking_repair: bool,
    pub tool_guardrail_mode: String,
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
    /// Pushed when a backend flips up<->down so the Uptime tab refreshes immediately.
    #[serde(rename = "backend_health_changed")]
    BackendHealthChanged {
        backend: String,
        status: String,
        latency_ms: Option<u64>,
    },
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
    /// Whether the request used SSE streaming.
    pub is_streaming: bool,
    /// Present only when the request failed; contains the error description.
    pub error_message: Option<String>,
    /// Stable operator-facing failure classification, such as `rate_limit`,
    /// `timeout`, or `invalid_request`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// Database row ID of the virtual key that authenticated this request.
    /// None when the request used a static API key or open relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<i64>,
    /// Estimated cost in USD for this request, computed from token usage
    /// and the model pricing table. None when cost could not be calculated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl SharedState {
    /// Construct a minimal SharedState for tests (in-memory DB, dummy channel).
    pub fn new_for_test() -> Self {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        crate::admin::db::init_db(&conn).expect("init_db");
        let hmac_secret = crate::admin::db::ensure_hmac_secret(&conn);
        let (events_tx, _) = broadcast::channel(4);
        let (log_tx, _) = tokio::sync::mpsc::channel(4);
        Self {
            db: Arc::new(Mutex::new(conn)),
            events_tx,
            runtime_config: Arc::new(RwLock::new(RuntimeConfig {
                model_mappings: IndexMap::new(),
                log_level: "info".to_string(),
                log_bodies: false,
                redact_secrets: false,
                anthropic_thinking_repair: false,
                tool_guardrail_mode: crate::tools::ToolGuardrailMode::Disabled
                    .as_str()
                    .to_string(),
            })),
            runtime_defaults: RuntimeConfigDefaults {
                log_bodies: false,
                redact_secrets: false,
                anthropic_thinking_repair: false,
                tool_guardrail_mode: crate::tools::ToolGuardrailMode::Disabled
                    .as_str()
                    .to_string(),
            },
            backend_metrics: Arc::new(HashMap::new()),
            log_tx,
            log_reload: None,
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys: Arc::new(DashMap::new()),
            hmac_secret: Arc::new(hmac_secret),
            model_router: None,
            provider_catalog: Arc::new(ProviderCatalog::bundled()),
            mcp_manager: None,
            issued_csrf_tokens: Arc::new(
                moka::sync::Cache::builder()
                    .max_capacity(1_000)
                    .time_to_live(Duration::from_secs(86400))
                    .build(),
            ),
            started_at: std::time::SystemTime::now(),
            managed_backends: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Aggregated metrics for the periodic WebSocket snapshot.
/// Matches the TypeScript `Metrics` interface — App.tsx feeds this directly into
/// the ['metrics'] react-query cache, so field names must stay in sync.
/// Latency percentiles are None from WS (computed on demand by the REST endpoint).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshotData {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub requests_per_minute: f64,
    pub p50_latency_ms: Option<u64>,
    pub p95_latency_ms: Option<u64>,
    pub error_rate: f64,
    pub streams_started: u64,
    pub streams_completed: u64,
    pub streams_failed: u64,
    pub streams_client_disconnected: u64,
}
