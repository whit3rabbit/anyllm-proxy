// SQLite schema, migrations, queries, and write buffer for request logging.

use crate::admin::state::RequestLogEntry;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Run an ALTER TABLE ADD COLUMN statement, ignoring "duplicate column" errors
/// so migrations are idempotent across restarts.
fn idempotent_add_column(conn: &Connection, stmt: &str) -> rusqlite::Result<()> {
    match conn.execute_batch(stmt) {
        Ok(()) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column") => Ok(()),
        Err(e) => Err(e),
    }
}

/// Initialize the SQLite database: create tables and indexes.
pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    // WAL mode: better read concurrency (proxy reads while admin writes)
    // and crash recovery compared to the default rollback journal.
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS request_log (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id      TEXT NOT NULL,
            timestamp       TEXT NOT NULL,
            backend         TEXT NOT NULL,
            model_requested TEXT,
            model_mapped    TEXT,
            status_code     INTEGER NOT NULL,
            latency_ms      INTEGER NOT NULL,
            input_tokens    INTEGER,
            output_tokens   INTEGER,
            is_streaming    INTEGER NOT NULL DEFAULT 0,
            error_message   TEXT,
            error_kind      TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_request_log_timestamp ON request_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_request_log_backend ON request_log(backend);
        CREATE INDEX IF NOT EXISTS idx_request_log_ts_latency ON request_log(timestamp, latency_ms);

        CREATE TABLE IF NOT EXISTS config_override (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS virtual_api_key (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            key_hash        TEXT NOT NULL UNIQUE,
            key_prefix      TEXT NOT NULL,
            description     TEXT,
            created_at      TEXT NOT NULL,
            expires_at      TEXT,
            revoked_at      TEXT,
            spend_limit     REAL,
            rpm_limit       INTEGER,
            tpm_limit       INTEGER,
            total_spend     REAL NOT NULL DEFAULT 0,
            total_requests  INTEGER NOT NULL DEFAULT 0,
            total_tokens    INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_vak_hash ON virtual_api_key(key_hash);

        CREATE TABLE IF NOT EXISTS audit_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL,
            action      TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id   TEXT,
            detail      TEXT,
            source_ip   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);

        ",
    )?;

    // Schema migrations for virtual_api_key new columns (idempotent via IF NOT EXISTS).
    // SQLite 3.37+ supports ADD COLUMN IF NOT EXISTS.
    let migration_stmts = [
        "ALTER TABLE virtual_api_key ADD COLUMN role TEXT NOT NULL DEFAULT 'developer'",
        "ALTER TABLE virtual_api_key ADD COLUMN max_budget_usd REAL",
        "ALTER TABLE virtual_api_key ADD COLUMN budget_duration TEXT",
        "ALTER TABLE virtual_api_key ADD COLUMN period_start TEXT",
        "ALTER TABLE virtual_api_key ADD COLUMN period_spend_usd REAL NOT NULL DEFAULT 0.0",
        "ALTER TABLE virtual_api_key ADD COLUMN total_input_tokens INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE virtual_api_key ADD COLUMN total_output_tokens INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE virtual_api_key ADD COLUMN allowed_models TEXT",
    ];
    for stmt in &migration_stmts {
        idempotent_add_column(conn, stmt)?;
    }

    // request_log migrations: add key_id and cost_usd for request attribution.
    let request_log_migrations = [
        "ALTER TABLE request_log ADD COLUMN key_id INTEGER",
        "ALTER TABLE request_log ADD COLUMN cost_usd REAL",
        "ALTER TABLE request_log ADD COLUMN error_kind TEXT",
    ];
    for stmt in &request_log_migrations {
        idempotent_add_column(conn, stmt)?;
    }

    // Index on key_id for filtering requests by virtual key.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_request_log_key_id ON request_log(key_id);",
    )?;

    Ok(())
}

/// Ensure an HMAC secret exists in the settings table. Creates one if missing.
/// Returns the 32-byte secret used for HMAC-SHA256 key hashing.
/// The secret is generated from two UUID v4s (uuid is already a dep) to avoid
/// adding a CSPRNG dependency; the entropy is sufficient for HMAC keying.
pub fn ensure_hmac_secret(conn: &Connection) -> Vec<u8> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL);",
    )
    .expect("create settings table");

    let existing: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'hmac_secret'",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(secret) = existing {
        return secret;
    }

    // Generate 256-bit CSPRNG secret directly.
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("CSPRNG failed");

    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('hmac_secret', ?1)",
        [&buf[..]],
    )
    .expect("insert hmac_secret");

    buf.to_vec()
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

// -- Config overrides --

/// Get all config overrides from SQLite.
pub fn get_config_overrides(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt =
        conn.prepare("SELECT key, value, updated_at FROM config_override ORDER BY key")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Set a config override (upsert).
pub fn set_config_override(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    let now = chrono_now();
    conn.execute(
        "INSERT INTO config_override (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
        params![key, value, now],
    )?;
    Ok(())
}

/// Delete a config override.
pub fn delete_config_override(conn: &Connection, key: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM config_override WHERE key = ?1", params![key])?;
    Ok(changed > 0)
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

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityBucket {
    pub bucket_start: String,
    pub requests_total: u64,
    pub requests_error: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

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

    // Fetch at most 2000 rows before Rust-side aggregation. Aggregation in SQL would require
    // a stable normalized-key column; doing it here avoids a schema change. Groups whose rows
    // span the cutoff may have incomplete counts, which is acceptable for the dashboard view.
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
        // Compute once; both the display summary and the group key derive from the same line.
        let first_line = first_failure_line(error_message.as_deref());
        let summary = truncate_for_display(&first_line, 120);
        let normalized = normalize_failure_group_key_from_line(&first_line);
        // U+001F (Unit Separator) is used as a field delimiter because it cannot appear
        // in backend names, model names, or normalized error tokens.
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
            avg_latency_ms: if aggregate.count == 0 {
                0
            } else {
                aggregate.total_latency_ms / aggregate.count
            },
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
        // This is safe because SQLite transactions provide ACID guarantees -- a panic
        // mid-transaction means the transaction was rolled back by SQLite.
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

/// ISO 8601 UTC timestamp for "now".
fn chrono_now() -> String {
    // Use std only, no chrono dependency. Format: 2026-03-22T10:15:30Z
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    epoch_to_iso8601(dur.as_secs())
}

/// Convert unix epoch seconds to ISO 8601 string (UTC, second precision).
pub(crate) fn epoch_to_iso8601(epoch: u64) -> String {
    // Manual conversion without chrono.
    let secs = epoch;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to year/month/day.
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert unix epoch milliseconds to ISO 8601 string with millisecond precision.
/// Format: "2026-03-27T10:15:30.500Z"
pub(crate) fn epoch_to_iso8601_ms(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let ms = epoch_ms % 1000;
    let base = epoch_to_iso8601(secs);
    // epoch_to_iso8601 returns "YYYY-MM-DDTHH:MM:SSZ"; strip the Z, append .mmmZ
    let without_z = base.trim_end_matches('Z');
    format!("{}.{:03}Z", without_z, ms)
}

/// Convert days since 1970-01-01 to (year, month, day).
pub(crate) fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Get the current time as ISO 8601 UTC string.
pub fn now_iso8601() -> String {
    chrono_now()
}

// --- Virtual API Key CRUD ---

use super::keys::VirtualKeyRow;

/// Parameters for creating a new virtual key.
pub struct InsertVirtualKeyParams<'a> {
    pub key_hash: &'a str,
    pub key_prefix: &'a str,
    pub description: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub spend_limit: Option<f64>,
    pub role: &'a str,
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<&'a str>,
    pub allowed_models: Option<String>,
}

/// Insert a new virtual API key.
pub fn insert_virtual_key(conn: &Connection, p: &InsertVirtualKeyParams) -> rusqlite::Result<i64> {
    let now = now_iso8601();
    conn.execute(
        "INSERT INTO virtual_api_key (key_hash, key_prefix, description, created_at, expires_at, \
         rpm_limit, tpm_limit, spend_limit, role, max_budget_usd, budget_duration, period_start, \
         allowed_models)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            p.key_hash,
            p.key_prefix,
            p.description,
            now,
            p.expires_at,
            p.rpm_limit.map(|v| v as i64),
            p.tpm_limit.map(|v| v as i64),
            p.spend_limit,
            p.role,
            p.max_budget_usd,
            p.budget_duration,
            // Set period_start to now if budget_duration is set
            p.budget_duration.map(|_| &now),
            p.allowed_models,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Map a SQLite row to a VirtualKeyRow.
fn row_to_virtual_key(row: &rusqlite::Row) -> rusqlite::Result<VirtualKeyRow> {
    Ok(VirtualKeyRow {
        id: row.get(0)?,
        key_hash: row.get(1)?,
        key_prefix: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        expires_at: row.get(5)?,
        revoked_at: row.get(6)?,
        rpm_limit: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        tpm_limit: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        spend_limit: row.get(9)?,
        total_spend: row.get::<_, f64>(10).unwrap_or(0.0),
        total_requests: row.get::<_, i64>(11).unwrap_or(0),
        total_tokens: row.get::<_, i64>(12).unwrap_or(0),
        role: row
            .get::<_, String>(13)
            .unwrap_or_else(|_| "developer".into()),
        max_budget_usd: row.get(14).unwrap_or(None),
        budget_duration: row.get(15).unwrap_or(None),
        period_start: row.get(16).unwrap_or(None),
        period_spend_usd: row.get::<_, f64>(17).unwrap_or(0.0),
        total_input_tokens: row.get::<_, i64>(18).unwrap_or(0),
        total_output_tokens: row.get::<_, i64>(19).unwrap_or(0),
        allowed_models: row
            .get::<_, Option<String>>(20)
            .unwrap_or(None)
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

const VIRTUAL_KEY_COLUMNS: &str =
    "id, key_hash, key_prefix, description, created_at, expires_at, revoked_at, \
     rpm_limit, tpm_limit, spend_limit, total_spend, total_requests, total_tokens, \
     role, max_budget_usd, budget_duration, period_start, period_spend_usd, \
     total_input_tokens, total_output_tokens, allowed_models";

/// List all virtual keys (active, expired, revoked).
pub fn list_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key ORDER BY id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_virtual_key)?;
    rows.collect()
}

/// Revoke a virtual key by setting revoked_at. Returns the row if found.
pub fn revoke_virtual_key(conn: &Connection, id: i64) -> rusqlite::Result<Option<VirtualKeyRow>> {
    let now = now_iso8601();
    let updated = conn.execute(
        "UPDATE virtual_api_key SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
        params![now, id],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![id], |row| Ok(Some(row_to_virtual_key(row)?)))
}

/// Parameters for updating an existing virtual key (all fields are optional; None = clear).
pub struct UpdateVirtualKeyParams<'a> {
    pub description: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<&'a str>,
    pub allowed_models: Option<String>,
}

/// Update an existing virtual key. Returns the updated row, or None if not found / revoked.
/// When `budget_duration` is provided, the budget period is reset (period_start = NULL,
/// period_spend_usd = 0) so the new window starts fresh.
pub fn update_virtual_key(
    conn: &Connection,
    id: i64,
    p: &UpdateVirtualKeyParams,
) -> rusqlite::Result<Option<VirtualKeyRow>> {
    // When changing budget_duration, reset the spend period so the new window starts clean.
    let mut sql = String::from(
        "UPDATE virtual_api_key
         SET description = ?2, expires_at = ?3, rpm_limit = ?4, tpm_limit = ?5,
             max_budget_usd = ?6, budget_duration = ?7, allowed_models = ?8",
    );
    if p.budget_duration.is_some() {
        sql.push_str(", period_start = NULL, period_spend_usd = 0.0");
    }
    sql.push_str(" WHERE id = ?1 AND revoked_at IS NULL");
    let updated = conn.execute(
        &sql,
        params![
            id,
            p.description,
            p.expires_at,
            p.rpm_limit.map(|v| v as i64),
            p.tpm_limit.map(|v| v as i64),
            p.max_budget_usd,
            p.budget_duration,
            p.allowed_models,
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![id], |row| Ok(Some(row_to_virtual_key(row)?)))
}

/// Load all active (non-revoked, non-expired) virtual keys from the database.
pub fn load_active_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let now = now_iso8601();
    let sql = format!(
        "SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key \
         WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?1)"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![now], row_to_virtual_key)?;
    rows.collect()
}

// --- Audit Log ---

/// A single audit log entry recording an admin mutation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub action: String,
    pub target_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
}

/// Insert an audit log entry with current UTC timestamp.
pub fn insert_audit_entry(conn: &Connection, entry: &AuditEntry) -> rusqlite::Result<()> {
    let ts = chrono_now();
    conn.execute(
        "INSERT INTO audit_log (timestamp, action, target_type, target_id, detail, source_ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ts,
            entry.action,
            entry.target_type,
            entry.target_id,
            entry.detail,
            entry.source_ip,
        ],
    )?;
    Ok(())
}

/// Query the audit log, returning entries in reverse chronological order.
pub fn query_audit_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    action: Option<&str>,
    target_type: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
) -> rusqlite::Result<Vec<AuditEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, action, target_type, target_id, detail, source_ip
         FROM audit_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(a) = action {
        sql.push_str(" AND action = ?");
        param_values.push(Box::new(a.to_string()));
    }
    if let Some(t) = target_type {
        sql.push_str(" AND target_type = ?");
        param_values.push(Box::new(t.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(u.to_string()));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(AuditEntry {
            id: Some(row.get(0)?),
            timestamp: Some(row.get(1)?),
            action: row.get(2)?,
            target_type: row.get(3)?,
            target_id: row.get(4)?,
            detail: row.get(5)?,
            source_ip: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn sample_entry() -> RequestLogEntry {
        RequestLogEntry {
            request_id: "test-123".into(),
            timestamp: "2099-01-01T00:00:00Z".into(),
            backend: "openai".into(),
            model_requested: Some("claude-sonnet-4-6".into()),
            model_mapped: Some("gpt-4o".into()),
            status_code: 200,
            latency_ms: 342,
            input_tokens: Some(150),
            output_tokens: Some(87),
            is_streaming: false,
            error_message: None,
            error_kind: None,
            key_id: None,
            cost_usd: None,
        }
    }

    #[test]
    fn init_db_creates_tables() {
        let conn = in_memory_db();
        // Verify tables exist by querying them.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM config_override", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn insert_and_query_request_log() {
        let conn = in_memory_db();
        let entry = sample_entry();
        insert_request_log(&conn, &entry).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_id, "test-123");
        assert_eq!(results[0].status_code, 200);
        assert_eq!(results[0].latency_ms, 342);
        assert_eq!(results[0].input_tokens, Some(150));
    }

    #[test]
    fn query_with_backend_filter() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let mut entry2 = sample_entry();
        entry2.request_id = "test-456".into();
        entry2.backend = "gemini".into();
        insert_request_log(&conn, &entry2).unwrap();

        let results =
            query_request_log(&conn, 10, 0, Some("gemini"), None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend, "gemini");
    }

    #[test]
    fn query_with_status_filter() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let mut err_entry = sample_entry();
        err_entry.request_id = "test-err".into();
        err_entry.status_code = 500;
        insert_request_log(&conn, &err_entry).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, Some("5xx"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 500);

        let results = query_request_log(&conn, 10, 0, None, None, None, Some("2xx"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 200);
    }

    #[test]
    fn query_pagination() {
        let conn = in_memory_db();
        for i in 0..5 {
            let mut entry = sample_entry();
            entry.request_id = format!("test-{i}");
            insert_request_log(&conn, &entry).unwrap();
        }

        let page1 = query_request_log(&conn, 2, 0, None, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = query_request_log(&conn, 2, 2, None, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = query_request_log(&conn, 2, 4, None, None, None, None, None).unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn get_request_by_id_found() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let result = get_request_by_id(&conn, "test-123").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().request_id, "test-123");
    }

    #[test]
    fn get_request_by_id_not_found() {
        let conn = in_memory_db();
        let result = get_request_by_id(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn config_override_crud() {
        let conn = in_memory_db();

        // Set
        set_config_override(&conn, "log_level", "debug").unwrap();
        let overrides = get_config_overrides(&conn).unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].0, "log_level");
        assert_eq!(overrides[0].1, "debug");

        // Update (upsert)
        set_config_override(&conn, "log_level", "trace").unwrap();
        let overrides = get_config_overrides(&conn).unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, "trace");

        // Delete
        let deleted = delete_config_override(&conn, "log_level").unwrap();
        assert!(deleted);
        let overrides = get_config_overrides(&conn).unwrap();
        assert!(overrides.is_empty());

        // Delete non-existent
        let deleted = delete_config_override(&conn, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn purge_old_logs_removes_old_entries() {
        let conn = in_memory_db();

        // Insert an old entry (timestamp in 2020).
        let mut old = sample_entry();
        old.timestamp = "2020-01-01T00:00:00Z".into();
        insert_request_log(&conn, &old).unwrap();

        // Insert a recent entry.
        insert_request_log(&conn, &sample_entry()).unwrap();

        let purged = purge_old_logs(&conn, 1).unwrap();
        assert_eq!(purged, 1);

        let remaining = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request_id, "test-123");
    }

    #[test]
    fn epoch_to_iso8601_known_value() {
        // 2026-03-22T00:00:00Z = 1774070400 (approximate)
        let result = epoch_to_iso8601(0);
        assert_eq!(result, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn epoch_to_iso8601_ms_formats_fractional_seconds() {
        assert_eq!(epoch_to_iso8601_ms(500), "1970-01-01T00:00:00.500Z");
        assert_eq!(epoch_to_iso8601_ms(1000), "1970-01-01T00:00:01.000Z");
        assert_eq!(epoch_to_iso8601_ms(1001), "1970-01-01T00:00:01.001Z");
        let result = epoch_to_iso8601_ms(1774070400000);
        assert!(result.ends_with(".000Z"), "got: {result}");
    }

    #[test]
    fn init_db_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // Running again should not error.
        init_db(&conn).unwrap();
    }

    #[test]
    fn insert_and_query_with_key_id_and_cost() {
        let conn = in_memory_db();
        let mut entry = sample_entry();
        entry.key_id = Some(42);
        entry.cost_usd = Some(0.0075);
        insert_request_log(&conn, &entry).unwrap();

        // Query without key_id filter returns all.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key_id, Some(42));
        assert!((results[0].cost_usd.unwrap() - 0.0075).abs() < 1e-12);

        // Query with matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(42)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_id, "test-123");

        // Query with non-matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(99)).unwrap();
        assert!(results.is_empty());

        // get_request_by_id also returns the new fields.
        let found = get_request_by_id(&conn, "test-123").unwrap().unwrap();
        assert_eq!(found.key_id, Some(42));
        assert!((found.cost_usd.unwrap() - 0.0075).abs() < 1e-12);
    }

    #[test]
    fn insert_without_attribution_fields() {
        // Entries without key_id/cost_usd should still work (NULL columns).
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key_id, None);
        assert_eq!(results[0].cost_usd, None);
    }

    #[test]
    fn audit_log_insert_and_query() {
        let conn = in_memory_db();
        let entry1 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_created".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: Some("description=test key, prefix=sk-vk-abc".into()),
            source_ip: Some("127.0.0.1".into()),
        };
        let entry2 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_revoked".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: None,
            source_ip: None,
        };
        insert_audit_entry(&conn, &entry1).unwrap();
        insert_audit_entry(&conn, &entry2).unwrap();

        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert_eq!(results.len(), 2);
        // Reverse chronological: most recent first.
        assert_eq!(results[0].action, "key_revoked");
        assert_eq!(results[1].action, "key_created");
        assert!(results[0].id.unwrap() > results[1].id.unwrap());
        // Timestamps are filled in by the insert function.
        assert!(results[0].timestamp.is_some());
        assert_eq!(results[1].target_id.as_deref(), Some("42"));
        assert_eq!(
            results[1].detail.as_deref(),
            Some("description=test key, prefix=sk-vk-abc")
        );
        assert_eq!(results[1].source_ip.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn audit_log_empty_returns_empty_vec() {
        let conn = in_memory_db();
        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn audit_log_pagination() {
        let conn = in_memory_db();
        for i in 0..5 {
            insert_audit_entry(
                &conn,
                &AuditEntry {
                    id: None,
                    timestamp: None,
                    action: format!("action_{i}"),
                    target_type: "test".into(),
                    target_id: None,
                    detail: None,
                    source_ip: None,
                },
            )
            .unwrap();
        }
        let page1 = query_audit_log(&conn, 2, 0, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);
        let page2 = query_audit_log(&conn, 2, 2, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);
        let page3 = query_audit_log(&conn, 2, 4, None, None, None, None).unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn status_filter_parses_valid_inputs() {
        assert!(StatusFilter::parse("200").is_some());
        assert!(StatusFilter::parse("2xx").is_some());
        assert!(StatusFilter::parse("4xx").is_some());
        assert!(StatusFilter::parse("5xx").is_some());
        assert!(StatusFilter::parse("404").is_some());
    }

    #[test]
    fn status_filter_rejects_invalid_inputs() {
        assert!(StatusFilter::parse("abc").is_none());
        assert!(StatusFilter::parse("2xx; DROP TABLE").is_none());
        assert!(StatusFilter::parse("").is_none());
        assert!(StatusFilter::parse("99999").is_none()); // overflows u16
        assert!(StatusFilter::parse("-1").is_none());
    }

    #[test]
    fn status_filter_exact_code_query() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap(); // status 200

        let mut err_entry = sample_entry();
        err_entry.request_id = "test-404".into();
        err_entry.status_code = 404;
        insert_request_log(&conn, &err_entry).unwrap();

        // Exact code filter should match only the 404 entry.
        let results = query_request_log(&conn, 10, 0, None, None, None, Some("404"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 404);
    }

    #[test]
    fn status_filter_invalid_ignored() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        // Invalid filter should be silently ignored, returning all rows.
        let results =
            query_request_log(&conn, 10, 0, None, None, None, Some("garbage"), None).unwrap();
        assert_eq!(results.len(), 1);
    }

    // --- update_virtual_key tests ---

    fn sample_key_params() -> InsertVirtualKeyParams<'static> {
        InsertVirtualKeyParams {
            key_hash: "hash-abc",
            key_prefix: "sk-vk-test",
            description: Some("test key"),
            expires_at: None,
            rpm_limit: Some(100),
            tpm_limit: None,
            spend_limit: None,
            role: "user",
            max_budget_usd: Some(10.0),
            budget_duration: Some("monthly"),
            allowed_models: None,
        }
    }

    #[test]
    fn update_virtual_key_returns_updated_row() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();

        let params = UpdateVirtualKeyParams {
            description: Some("updated desc"),
            expires_at: None,
            rpm_limit: Some(200),
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: None,
        };
        let row = update_virtual_key(&conn, id, &params).unwrap();
        assert!(row.is_some());
        let row = row.unwrap();
        assert_eq!(row.description.as_deref(), Some("updated desc"));
        assert_eq!(row.rpm_limit, Some(200));
    }

    #[test]
    fn update_virtual_key_on_revoked_returns_none() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();
        revoke_virtual_key(&conn, id).unwrap();

        let params = UpdateVirtualKeyParams {
            description: Some("should not apply"),
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: None,
        };
        let row = update_virtual_key(&conn, id, &params).unwrap();
        assert!(row.is_none());
    }

    #[test]
    fn update_virtual_key_allowed_models_roundtrip() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();

        let models_json = serde_json::to_string(&["gpt-4o", "claude-*"]).unwrap();
        let params = UpdateVirtualKeyParams {
            description: None,
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: Some(models_json),
        };
        let row = update_virtual_key(&conn, id, &params).unwrap().unwrap();
        // row.allowed_models is parsed from JSON into Vec<String>.
        assert_eq!(
            row.allowed_models,
            Some(vec!["gpt-4o".to_string(), "claude-*".to_string()])
        );
    }

    #[test]
    fn update_virtual_key_budget_duration_resets_period() {
        let conn = in_memory_db();
        // Insert with a non-null period_spend_usd to verify reset.
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();
        conn.execute(
            "UPDATE virtual_api_key SET period_spend_usd = 5.0, period_start = '2020-01-01' WHERE id = ?1",
            params![id],
        )
        .unwrap();

        let params = UpdateVirtualKeyParams {
            description: None,
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: Some("daily"),
            allowed_models: None,
        };
        update_virtual_key(&conn, id, &params).unwrap();

        // period_spend_usd should be reset to 0, period_start to NULL.
        let (spend, start): (f64, Option<String>) = conn
            .query_row(
                "SELECT period_spend_usd, period_start FROM virtual_api_key WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(spend, 0.0);
        assert!(start.is_none());
    }

    // --- query_audit_log filter tests ---

    fn insert_audit(conn: &Connection, action: &str, target_type: &str, ts: &str) {
        conn.execute(
            "INSERT INTO audit_log (timestamp, action, source_ip, target_type, target_id, detail) \
             VALUES (?1, ?2, '127.0.0.1', ?3, NULL, NULL)",
            params![ts, action, target_type],
        )
        .unwrap();
    }

    #[test]
    fn audit_filter_by_action() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, Some("key_created"), None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_created");
    }

    #[test]
    fn audit_filter_by_target_type() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "config_changed", "config", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, None, Some("config"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target_type, "config");
    }

    #[test]
    fn audit_filter_since_until() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-03T00:00:00Z");
        insert_audit(&conn, "key_updated", "virtual_key", "2099-01-05T00:00:00Z");

        // since + until window should return only the middle entry.
        let results = query_audit_log(
            &conn,
            10,
            0,
            None,
            None,
            Some("2099-01-02T00:00:00Z"),
            Some("2099-01-04T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_revoked");
    }

    #[test]
    fn count_requests_since_returns_zero_on_empty_log() {
        let conn = in_memory_db();
        let count = count_requests_since(&conn, 0).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn count_requests_since_counts_recent_entries() {
        let conn = in_memory_db();

        // Insert a recent entry (sample_entry uses current time).
        let recent = sample_entry();
        insert_request_log(&conn, &recent).unwrap();

        // Insert an old entry.
        let mut old = sample_entry();
        old.request_id = "old-req".to_string();
        old.timestamp = "2020-01-01T00:00:00Z".to_string();
        insert_request_log(&conn, &old).unwrap();

        // Count since 2025-01-01 should include only the recent entry.
        let since_2025: u64 = 1735689600; // 2025-01-01T00:00:00Z
        let count = count_requests_since(&conn, since_2025).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn observability_timeseries_groups_requests_into_buckets() {
        let conn = in_memory_db();

        let mut first = sample_entry();
        first.timestamp = "2099-01-01T00:00:05Z".into();
        first.input_tokens = Some(100);
        first.output_tokens = Some(25);
        first.cost_usd = Some(0.12);
        insert_request_log(&conn, &first).unwrap();

        let mut second = sample_entry();
        second.request_id = "test-456".into();
        second.timestamp = "2099-01-01T00:00:40Z".into();
        second.status_code = 503;
        second.input_tokens = Some(20);
        second.output_tokens = Some(4);
        second.cost_usd = Some(0.03);
        second.error_message = Some("upstream timeout".into());
        insert_request_log(&conn, &second).unwrap();

        let mut third = sample_entry();
        third.request_id = "test-789".into();
        third.timestamp = "2099-01-01T00:01:10Z".into();
        third.input_tokens = Some(7);
        third.output_tokens = Some(9);
        third.cost_usd = Some(0.01);
        insert_request_log(&conn, &third).unwrap();

        let buckets = query_request_timeseries(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T00:10:00Z"),
            None,
            None,
        )
        .unwrap();

        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].bucket_start, "2099-01-01T00:00:00Z");
        assert_eq!(buckets[0].requests_total, 2);
        assert_eq!(buckets[0].requests_error, 1);
        assert_eq!(buckets[0].input_tokens, 120);
        assert_eq!(buckets[0].output_tokens, 29);
        assert!((buckets[0].cost_usd - 0.15).abs() < 0.000001);
        assert_eq!(buckets[1].bucket_start, "2099-01-01T00:01:00Z");
        assert_eq!(buckets[1].requests_total, 1);
    }

    #[test]
    fn observability_timeline_derives_request_start_time() {
        let conn = in_memory_db();

        let mut entry = sample_entry();
        entry.timestamp = "2099-01-01T00:00:10Z".into();
        entry.latency_ms = 1_500;
        insert_request_log(&conn, &entry).unwrap();

        let items = query_request_timeline(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T00:05:00Z"),
            None,
            None,
            10,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].finished_at, "2099-01-01T00:00:10.000Z");
        assert_eq!(items[0].started_at, "2099-01-01T00:00:08.500Z");
    }

    #[test]
    fn observability_failure_breakdown_groups_similar_failures() {
        let conn = in_memory_db();

        let mut first = sample_entry();
        first.request_id = "test-fail-1".into();
        first.timestamp = "2099-01-01T00:00:10Z".into();
        first.status_code = 429;
        first.latency_ms = 500;
        first.error_message = Some("Upstream request req_abc123 throttled after 30s".into());
        first.error_kind = Some("rate_limit".into());
        insert_request_log(&conn, &first).unwrap();

        let mut second = sample_entry();
        second.request_id = "test-fail-2".into();
        second.timestamp = "2099-01-01T00:00:20Z".into();
        second.status_code = 429;
        second.latency_ms = 700;
        second.error_message = Some("Upstream request req_xyz789 throttled after 45s".into());
        second.error_kind = Some("rate_limit".into());
        insert_request_log(&conn, &second).unwrap();

        let mut third = sample_entry();
        third.request_id = "test-fail-3".into();
        third.timestamp = "2099-01-01T00:00:30Z".into();
        third.status_code = 500;
        third.error_message = Some("Backend crashed".into());
        third.error_kind = Some("upstream".into());
        insert_request_log(&conn, &third).unwrap();

        let mut fourth = sample_entry();
        fourth.request_id = "test-fail-4".into();
        fourth.timestamp = "2099-01-01T00:00:40Z".into();
        fourth.status_code = 429;
        fourth.error_message = Some("Upstream request req_qwe999 throttled after 60s".into());
        fourth.error_kind = Some("timeout".into());
        insert_request_log(&conn, &fourth).unwrap();

        let failures = query_failure_breakdown(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T01:00:00Z"),
            None,
            None,
            10,
        )
        .unwrap();

        assert_eq!(failures.len(), 3);
        assert_eq!(failures[0].error_kind.as_deref(), Some("rate_limit"));
        assert_eq!(failures[0].status_code, 429);
        assert_eq!(failures[0].count, 2);
        assert_eq!(failures[0].avg_latency_ms, 600);
        assert!(failures[0].summary.starts_with("Upstream request"));
    }
}
