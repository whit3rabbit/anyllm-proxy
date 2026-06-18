use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::common::{epoch_to_iso8601, epoch_to_iso8601_ms};
use crate::admin::state::RequestLogEntry;

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

/// Query request log with optional filters and pagination.
/// Typed status code filter -- prevents SQL injection by construction.
/// Only valid patterns are representable; invalid input is rejected at parse time.
enum StatusFilter {
    Exact(u16),
    Class2xx,
    Class4xx,
    Class5xx,
}

impl StatusFilter {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "2xx" => Some(Self::Class2xx),
            "4xx" => Some(Self::Class4xx),
            "5xx" => Some(Self::Class5xx),
            other => other.parse::<u16>().ok().map(Self::Exact),
        }
    }

    fn apply_to_query(&self, sql: &mut String, params: &mut Vec<Box<dyn rusqlite::types::ToSql>>) {
        match self {
            Self::Exact(code) => {
                sql.push_str(" AND status_code = ?");
                params.push(Box::new(*code as i64));
            }
            Self::Class2xx => sql.push_str(" AND status_code >= 200 AND status_code < 300"),
            Self::Class4xx => sql.push_str(" AND status_code >= 400 AND status_code < 500"),
            Self::Class5xx => sql.push_str(" AND status_code >= 500 AND status_code < 600"),
        }
    }
}

/// Query the request log with optional filters. Returns rows newest-first.
#[allow(clippy::too_many_arguments)]
pub fn query_request_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    backend: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    status_filter: Option<&str>,
    key_id: Option<i64>,
) -> rusqlite::Result<Vec<RequestLogEntry>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd
         FROM request_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(b) = backend {
        sql.push_str(" AND backend = ?");
        param_values.push(Box::new(b.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(u.to_string()));
    }
    if let Some(sf) = status_filter {
        if let Some(parsed) = StatusFilter::parse(sf) {
            parsed.apply_to_query(&mut sql, &mut param_values);
        }
        // Invalid filter silently ignored
    }
    if let Some(kid) = key_id {
        sql.push_str(" AND key_id = ?");
        param_values.push(Box::new(kid));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_request_log)?;

    rows.collect()
}

/// Map a SQLite row to a RequestLogEntry. Column order must match the SELECT
/// used in query_request_log and get_request_by_id.
fn row_to_request_log(row: &rusqlite::Row) -> rusqlite::Result<RequestLogEntry> {
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

/// Append the optional `until`, `backend`, and `key_id` WHERE clauses shared by all
/// observability queries. `params` must already contain the `since` binding as `?1`.
fn append_common_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
) {
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        params.push(Box::new(u.to_string()));
    }
    if let Some(b) = backend {
        sql.push_str(" AND backend = ?");
        params.push(Box::new(b.to_string()));
    }
    if let Some(kid) = key_id {
        sql.push_str(" AND key_id = ?");
        params.push(Box::new(kid));
    }
}

/// Aggregate request log into 1-minute buckets for the timeseries chart.
pub fn query_request_timeseries(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
) -> rusqlite::Result<Vec<ObservabilityBucket>> {
    let mut sql = String::from(
        "SELECT strftime('%Y-%m-%dT%H:%M:00Z', timestamp) AS bucket_start,
                COUNT(*) AS requests_total,
                SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) AS requests_error,
                SUM(COALESCE(input_tokens, 0)) AS input_tokens,
                SUM(COALESCE(output_tokens, 0)) AS output_tokens,
                SUM(COALESCE(cost_usd, 0.0)) AS cost_usd
         FROM request_log
         WHERE timestamp >= ?",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    sql.push_str(" GROUP BY bucket_start ORDER BY bucket_start ASC");

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(ObservabilityBucket {
            bucket_start: row.get(0)?,
            requests_total: row.get::<_, i64>(1)?.max(0) as u64,
            requests_error: row.get::<_, i64>(2)?.max(0) as u64,
            input_tokens: row.get::<_, i64>(3)?.max(0) as u64,
            output_tokens: row.get::<_, i64>(4)?.max(0) as u64,
            cost_usd: row.get::<_, f64>(5).unwrap_or(0.0),
        })
    })?;
    rows.collect()
}

/// Fetch individual request entries for the waterfall timeline view (newest first).
pub fn query_request_timeline(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<ObservabilityTimelineItem>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped, status_code,
                latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd, CAST(strftime('%s', timestamp) AS INTEGER) * 1000
         FROM request_log
         WHERE timestamp >= ?",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    sql.push_str(" ORDER BY timestamp DESC LIMIT ?");
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let finished_at_ms = row.get::<_, i64>(14)?.max(0) as u64;
        let latency_ms = row.get::<_, i64>(6)?.max(0) as u64;
        let started_at_ms = finished_at_ms.saturating_sub(latency_ms);
        let model_requested: Option<String> = row.get(3)?;
        let model_mapped: Option<String> = row.get(4)?;
        Ok(ObservabilityTimelineItem {
            request_id: row.get(0)?,
            started_at: epoch_to_iso8601_ms(started_at_ms),
            finished_at: epoch_to_iso8601_ms(finished_at_ms),
            backend: row.get(2)?,
            model: model_mapped.or(model_requested),
            status_code: row.get::<_, i64>(5)?.max(0) as u16,
            latency_ms,
            input_tokens: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
            output_tokens: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
            is_streaming: row.get::<_, i64>(9)? != 0,
            error_message: row.get(10)?,
            error_kind: row.get(11)?,
            key_id: row.get(12)?,
            cost_usd: row.get(13)?,
        })
    })?;
    rows.collect()
}

/// Group recent failures by (error_kind, backend, model, status_code) for the failure-breakdown panel.
pub fn query_failure_breakdown(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<ObservabilityFailureItem>> {
    let mut sql = String::from(
        "SELECT timestamp, backend, model_requested, model_mapped, status_code,
                latency_ms, error_message, error_kind
         FROM request_log
         WHERE timestamp >= ? AND status_code >= 400",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    // Fetch at most 2000 rows before Rust-side aggregation.
    sql.push_str(" ORDER BY timestamp DESC LIMIT 2000");

    #[derive(Debug)]
    struct FailureAggregate {
        error_kind: Option<String>,
        backend: String,
        model: Option<String>,
        status_code: u16,
        count: u64,
        latest_seen: String,
        total_latency_ms: u64,
        summary: String,
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let model_requested: Option<String> = row.get(2)?;
        let model_mapped: Option<String> = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            model_mapped.or(model_requested),
            row.get::<_, i64>(4)?.max(0) as u16,
            row.get::<_, i64>(5)?.max(0) as u64,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;

    let mut grouped = HashMap::<String, FailureAggregate>::new();
    for row in rows {
        let (timestamp, backend_name, model, status_code, latency_ms, error_message, error_kind) =
            row?;
        let first_line = first_failure_line(error_message.as_deref());
        let summary = truncate_for_display(&first_line, 120);
        let normalized = normalize_failure_group_key_from_line(&first_line);
        let group_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            backend_name,
            status_code,
            model.clone().unwrap_or_default(),
            error_kind.clone().unwrap_or_default(),
            normalized
        );

        let entry = grouped
            .entry(group_key)
            .or_insert_with(|| FailureAggregate {
                error_kind: error_kind.clone(),
                backend: backend_name.clone(),
                model: model.clone(),
                status_code,
                count: 0,
                latest_seen: timestamp.clone(),
                total_latency_ms: 0,
                summary: summary.clone(),
            });
        entry.count += 1;
        entry.total_latency_ms = entry.total_latency_ms.saturating_add(latency_ms);
        if timestamp >= entry.latest_seen {
            entry.latest_seen = timestamp;
            entry.summary = summary;
        }
    }

    let mut failures = grouped
        .into_values()
        .map(|aggregate| ObservabilityFailureItem {
            error_kind: aggregate.error_kind,
            backend: aggregate.backend,
            model: aggregate.model,
            status_code: aggregate.status_code,
            count: aggregate.count,
            latest_seen: aggregate.latest_seen,
            avg_latency_ms: aggregate
                .total_latency_ms
                .checked_div(aggregate.count)
                .unwrap_or(0),
            summary: aggregate.summary,
        })
        .collect::<Vec<_>>();

    failures.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest_seen.cmp(&left.latest_seen))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    failures.truncate(limit as usize);
    Ok(failures)
}

fn first_failure_line(message: Option<&str>) -> String {
    collapse_whitespace(
        message
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("Unknown failure"),
    )
}

fn collapse_whitespace(input: &str) -> String {
    let mut collapsed = String::with_capacity(input.len());
    let mut previous_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                collapsed.push(' ');
                previous_was_space = true;
            }
        } else {
            collapsed.push(ch);
            previous_was_space = false;
        }
    }
    collapsed.trim().to_string()
}

fn truncate_for_display(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn normalize_failure_group_key_from_line(first_line: &str) -> String {
    let lowercase = first_line.to_ascii_lowercase();
    let tokens = lowercase
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
            if trimmed.is_empty() {
                None
            } else {
                Some(normalize_failure_token(trimmed))
            }
        })
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        "<empty>".to_string()
    } else {
        tokens.join(" ")
    }
}

fn normalize_failure_token(token: &str) -> String {
    if looks_like_id(token) {
        "<id>".to_string()
    } else if looks_like_numberish(token) {
        "<num>".to_string()
    } else {
        token.to_string()
    }
}

fn looks_like_numberish(token: &str) -> bool {
    fn is_numericish(input: &str) -> bool {
        !input.is_empty()
            && input
                .chars()
                .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ':' | '/' | '%'))
    }

    if is_numericish(token) {
        return true;
    }

    for suffix in ["ms", "s", "sec", "secs"] {
        if let Some(prefix) = token.strip_suffix(suffix) {
            return is_numericish(prefix);
        }
    }

    false
}

fn looks_like_id(token: &str) -> bool {
    let lowercase = token.to_ascii_lowercase();
    if [
        "req_",
        "msg_",
        "run_",
        "resp_",
        "call_",
        "toolu_",
        "chatcmpl-",
        "cmpl-",
    ]
    .iter()
    .any(|prefix| lowercase.starts_with(prefix))
    {
        return true;
    }

    let compact = lowercase.replace('-', "");
    if compact.len() >= 24 && compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }

    // Single pass: check all three conditions simultaneously.
    lowercase.len() >= 16 && {
        let mut has_alpha = false;
        let mut has_digit = false;
        let all_valid = lowercase.chars().all(|ch| {
            if ch.is_ascii_alphabetic() {
                has_alpha = true;
            } else if ch.is_ascii_digit() {
                has_digit = true;
            }
            ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
        });
        all_valid && has_alpha && has_digit
    }
}

/// Spawn the write buffer background task. Returns the sender for proxy handlers.
/// Flushes every 100ms or 100 rows, whichever comes first.
pub fn spawn_write_buffer(db: Arc<Mutex<Connection>>) -> mpsc::Sender<RequestLogEntry> {
    let (tx, mut rx) = mpsc::channel::<RequestLogEntry>(1024);

    tokio::spawn(async move {
        let mut buf: Vec<RequestLogEntry> = Vec::with_capacity(128);
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                maybe_entry = rx.recv() => {
                    match maybe_entry {
                        Some(entry) => {
                            buf.push(entry);
                            if buf.len() >= 100 {
                                flush_buffer(&db, &mut buf).await;
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit.
                            if !buf.is_empty() {
                                flush_buffer(&db, &mut buf).await;
                            }
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buf.is_empty() {
                        flush_buffer(&db, &mut buf).await;
                    }
                }
            }
        }
    });

    tx
}

async fn flush_buffer(db: &Arc<Mutex<Connection>>, buf: &mut Vec<RequestLogEntry>) {
    let entries = std::mem::take(buf);
    let db = db.clone();
    // Run SQLite IO on the blocking threadpool to avoid stalling the tokio executor.
    // On failure, return the entries so they can be re-queued for retry.
    let result = tokio::task::spawn_blocking(move || {
        // Mutex poisoning recovery: if a prior request panicked while holding the lock,
        // we recover the inner value rather than permanently locking the database.
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = (|| -> rusqlite::Result<()> {
            let tx = conn.unchecked_transaction()?;
            for entry in &entries {
                insert_request_log(&tx, entry)?;
            }
            tx.commit()?;
            Ok(())
        })() {
            tracing::error!(error = %e, count = entries.len(), "failed to flush request log buffer");
            Some(entries)
        } else {
            None
        }
    })
    .await;

    // On failure, re-queue entries so they can be retried on the next flush.
    if let Ok(Some(mut entries)) = result {
        buf.append(&mut entries);
        // Cap retry buffer to prevent unbounded growth on persistent DB failure.
        const MAX_RETRY_BUFFER: usize = 1000;
        if buf.len() > MAX_RETRY_BUFFER {
            let dropped = buf.len() - MAX_RETRY_BUFFER;
            buf.drain(..dropped);
            tracing::warn!(dropped, "dropped oldest log entries to cap retry buffer");
        }
    }
}

#[cfg(test)]
mod tests;
