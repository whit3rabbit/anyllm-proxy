use rusqlite::{params, Connection};

use crate::admin::db::common::epoch_to_iso8601;
use crate::admin::state::RequestLogEntry;

pub mod failure_normalization;
pub mod queries;
pub mod write_buffer;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use queries::StatusFilter;
pub use queries::{
    query_failure_breakdown, query_request_log, query_request_timeline, query_request_timeseries,
};
pub use write_buffer::spawn_write_buffer;

/// One time-bucket in the request timeseries (1-minute granularity).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityBucket {
    pub bucket_start: String,
    #[serde(rename = "requests")]
    pub requests_total: u64,
    #[serde(rename = "errors")]
    pub requests_error: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// A single request entry in the waterfall timeline view.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityTimelineItem {
    pub request_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub backend: String,
    pub model: Option<String>,
    pub status_code: u16,
    pub latency_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub is_streaming: bool,
    pub key_id: Option<i64>,
    pub cost_usd: Option<f64>,
    pub error_message: Option<String>,
    pub error_kind: Option<String>,
}

/// An aggregated failure group for the failure-breakdown panel.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityFailureItem {
    pub error_kind: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub status_code: u16,
    pub count: u64,
    pub latest_seen: String,
    pub avg_latency_ms: u64,
    pub summary: String,
}

/// Insert a single request log entry.
pub fn insert_request_log(conn: &Connection, entry: &RequestLogEntry) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO request_log (
            request_id, timestamp, backend, model_requested, model_mapped,
            status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
            error_kind, key_id, cost_usd
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            entry.request_id,
            entry.timestamp,
            entry.backend,
            entry.model_requested,
            entry.model_mapped,
            entry.status_code,
            entry.latency_ms,
            entry.input_tokens.map(|v| v as i64),
            entry.output_tokens.map(|v| v as i64),
            entry.is_streaming as i32,
            entry.error_message,
            entry.error_kind,
            entry.key_id,
            entry.cost_usd,
        ],
    )?;
    Ok(())
}

/// Map a SQLite row to a RequestLogEntry. Column order must match the SELECT
/// used in query_request_log and get_request_by_id.
pub(crate) fn row_to_request_log(row: &rusqlite::Row) -> rusqlite::Result<RequestLogEntry> {
    Ok(RequestLogEntry {
        request_id: row.get(0)?,
        timestamp: row.get(1)?,
        backend: row.get(2)?,
        model_requested: row.get(3)?,
        model_mapped: row.get(4)?,
        status_code: row.get::<_, i32>(5)? as u16,
        latency_ms: row.get::<_, i64>(6)? as u64,
        input_tokens: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        output_tokens: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
        is_streaming: row.get::<_, i32>(9)? != 0,
        error_message: row.get(10)?,
        error_kind: row.get(11)?,
        key_id: row.get(12)?,
        cost_usd: row.get(13)?,
    })
}

/// Get a single request log entry by request_id.
pub fn get_request_by_id(
    conn: &Connection,
    request_id: &str,
) -> rusqlite::Result<Option<RequestLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd
         FROM request_log WHERE request_id = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![request_id], row_to_request_log)?;
    rows.next().transpose()
}

/// Delete request log entries older than the given number of days.
pub fn purge_old_logs(conn: &Connection, retention_days: u32) -> rusqlite::Result<usize> {
    // SQLite datetime comparison: delete rows where timestamp < cutoff
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(retention_days as u64 * 86400);
    let cutoff_iso = epoch_to_iso8601(cutoff);
    let changed = conn.execute(
        "DELETE FROM request_log WHERE timestamp < ?1",
        params![cutoff_iso],
    )?;
    Ok(changed)
}

/// Count request log entries with a timestamp >= `since_epoch` (Unix seconds).
/// Used to compute requests-per-second for the metrics dashboard.
pub fn count_requests_since(conn: &Connection, since_epoch: u64) -> rusqlite::Result<u64> {
    let since_iso = epoch_to_iso8601(since_epoch);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM request_log WHERE timestamp >= ?1",
        rusqlite::params![since_iso],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}
