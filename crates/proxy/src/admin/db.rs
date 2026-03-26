// SQLite schema, migrations, queries, and write buffer for request logging.

use crate::admin::state::RequestLogEntry;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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
            error_message   TEXT
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
        ",
    )?;
    Ok(())
}

/// Insert a single request log entry.
pub fn insert_request_log(conn: &Connection, entry: &RequestLogEntry) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO request_log (
            request_id, timestamp, backend, model_requested, model_mapped,
            status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
        ],
    )?;
    Ok(())
}

/// Query request log with optional filters and pagination.
pub fn query_request_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    backend: Option<&str>,
    since: Option<&str>,
    status_filter: Option<&str>,
) -> rusqlite::Result<Vec<RequestLogEntry>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message
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
    if let Some(sf) = status_filter {
        // Support "5xx", "4xx", "2xx" patterns or exact status code
        match sf {
            "2xx" => sql.push_str(" AND status_code >= 200 AND status_code < 300"),
            "4xx" => sql.push_str(" AND status_code >= 400 AND status_code < 500"),
            "5xx" => sql.push_str(" AND status_code >= 500 AND status_code < 600"),
            exact => {
                sql.push_str(" AND status_code = ?");
                param_values.push(Box::new(exact.to_string()));
            }
        }
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
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
        })
    })?;

    rows.collect()
}

/// Get a single request log entry by request_id.
pub fn get_request_by_id(
    conn: &Connection,
    request_id: &str,
) -> rusqlite::Result<Option<RequestLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message
         FROM request_log WHERE request_id = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![request_id], |row| {
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
        })
    })?;
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

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
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

/// Insert a new virtual API key.
#[allow(clippy::too_many_arguments)]
pub fn insert_virtual_key(
    conn: &Connection,
    key_hash: &str,
    key_prefix: &str,
    description: Option<&str>,
    expires_at: Option<&str>,
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    spend_limit: Option<f64>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO virtual_api_key (key_hash, key_prefix, description, created_at, expires_at, rpm_limit, tpm_limit, spend_limit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            key_hash,
            key_prefix,
            description,
            now_iso8601(),
            expires_at,
            rpm_limit.map(|v| v as i64),
            tpm_limit.map(|v| v as i64),
            spend_limit,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// List all virtual keys (active, expired, revoked).
pub fn list_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, key_hash, key_prefix, description, created_at, expires_at, revoked_at,
                rpm_limit, tpm_limit, spend_limit, total_spend, total_requests, total_tokens
         FROM virtual_api_key ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
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
        })
    })?;
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
    let mut stmt = conn.prepare(
        "SELECT id, key_hash, key_prefix, description, created_at, expires_at, revoked_at,
                rpm_limit, tpm_limit, spend_limit, total_spend, total_requests, total_tokens
         FROM virtual_api_key WHERE id = ?1",
    )?;
    stmt.query_row(params![id], |row| {
        Ok(Some(VirtualKeyRow {
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
        }))
    })
}

/// Load all active (non-revoked, non-expired) virtual keys from the database.
pub fn load_active_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let now = now_iso8601();
    let mut stmt = conn.prepare(
        "SELECT id, key_hash, key_prefix, description, created_at, expires_at, revoked_at,
                rpm_limit, tpm_limit, spend_limit, total_spend, total_requests, total_tokens
         FROM virtual_api_key
         WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?1)",
    )?;
    let rows = stmt.query_map(params![now], |row| {
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

        let results = query_request_log(&conn, 10, 0, None, None, None).unwrap();
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

        let results = query_request_log(&conn, 10, 0, Some("gemini"), None, None).unwrap();
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

        let results = query_request_log(&conn, 10, 0, None, None, Some("5xx")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 500);

        let results = query_request_log(&conn, 10, 0, None, None, Some("2xx")).unwrap();
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

        let page1 = query_request_log(&conn, 2, 0, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = query_request_log(&conn, 2, 2, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = query_request_log(&conn, 2, 4, None, None, None).unwrap();
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

        let remaining = query_request_log(&conn, 10, 0, None, None, None).unwrap();
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
    fn init_db_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // Running again should not error.
        init_db(&conn).unwrap();
    }
}
