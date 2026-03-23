// Shared state between the proxy and admin server.
// RuntimeConfig holds mutable settings; AdminEvent is broadcast to WebSocket clients.

use crate::config::ModelMapping;
use crate::metrics::Metrics;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

/// Shared between proxy handlers and the admin server.
#[derive(Clone)]
pub struct SharedState {
    /// SQLite connection for request logging and config persistence.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    /// Broadcast channel sender for live dashboard updates.
    pub events_tx: broadcast::Sender<AdminEvent>,
    /// Runtime-mutable config. The proxy reads this on each request.
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    /// Per-backend metrics (same Arc the proxy already uses).
    pub backend_metrics: Arc<HashMap<String, Metrics>>,
    /// Write buffer sender for batched SQLite inserts.
    pub log_tx: tokio::sync::mpsc::Sender<RequestLogEntry>,
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

/// Data recorded for each proxied request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestLogEntry {
    pub request_id: String,
    pub timestamp: String,
    pub backend: String,
    pub model_requested: Option<String>,
    pub model_mapped: Option<String>,
    pub status_code: u16,
    pub latency_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub is_streaming: bool,
    pub error_message: Option<String>,
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
