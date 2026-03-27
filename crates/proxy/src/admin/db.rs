// SQLite schema, migrations, queries, and write buffer for request logging.

use crate::admin::state::RequestLogEntry;
use rusqlite::{params, Connection};
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

        CREATE TABLE IF NOT EXISTS batch_file (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id     TEXT NOT NULL UNIQUE,
            key_id      INTEGER,
            purpose     TEXT NOT NULL,
            filename    TEXT,
            byte_size   INTEGER NOT NULL,
            line_count  INTEGER NOT NULL,
            content     BLOB NOT NULL,
            created_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS batch_job (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id                 TEXT NOT NULL UNIQUE,
            key_id                   INTEGER,
            input_file_id            TEXT NOT NULL,
            backend_batch_id         TEXT,
            backend_name             TEXT NOT NULL,
            status                   TEXT NOT NULL,
            request_counts_total     INTEGER NOT NULL DEFAULT 0,
            request_counts_completed INTEGER NOT NULL DEFAULT 0,
            request_counts_failed    INTEGER NOT NULL DEFAULT 0,
            output_file_id           TEXT,
            error_file_id            TEXT,
            created_at               TEXT NOT NULL,
            completed_at             TEXT,
            expires_at               TEXT,
            metadata                 TEXT
        );
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

    // Generate 32 random bytes from two UUID v4s.
    let mut buf = [0u8; 32];
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    buf[..16].copy_from_slice(a.as_bytes());
    buf[16..].copy_from_slice(b.as_bytes());

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
            key_id, cost_usd
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            entry.key_id,
            entry.cost_usd,
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
    key_id: Option<i64>,
) -> rusqlite::Result<Vec<RequestLogEntry>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                key_id, cost_usd
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
        key_id: row.get(11)?,
        cost_usd: row.get(12)?,
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
                key_id, cost_usd
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
) -> rusqlite::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, action, target_type, target_id, detail, source_ip
         FROM audit_log ORDER BY id DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![limit, offset], |row| {
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

        let results = query_request_log(&conn, 10, 0, None, None, None, None).unwrap();
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

        let results = query_request_log(&conn, 10, 0, Some("gemini"), None, None, None).unwrap();
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

        let results = query_request_log(&conn, 10, 0, None, None, Some("5xx"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 500);

        let results = query_request_log(&conn, 10, 0, None, None, Some("2xx"), None).unwrap();
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

        let page1 = query_request_log(&conn, 2, 0, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = query_request_log(&conn, 2, 2, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = query_request_log(&conn, 2, 4, None, None, None, None).unwrap();
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

        let remaining = query_request_log(&conn, 10, 0, None, None, None, None).unwrap();
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

    #[test]
    fn insert_and_query_with_key_id_and_cost() {
        let conn = in_memory_db();
        let mut entry = sample_entry();
        entry.key_id = Some(42);
        entry.cost_usd = Some(0.0075);
        insert_request_log(&conn, &entry).unwrap();

        // Query without key_id filter returns all.
        let results = query_request_log(&conn, 10, 0, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key_id, Some(42));
        assert!((results[0].cost_usd.unwrap() - 0.0075).abs() < 1e-12);

        // Query with matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, Some(42)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_id, "test-123");

        // Query with non-matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, Some(99)).unwrap();
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

        let results = query_request_log(&conn, 10, 0, None, None, None, None).unwrap();
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

        let results = query_audit_log(&conn, 50, 0).unwrap();
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
        let results = query_audit_log(&conn, 50, 0).unwrap();
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
        let page1 = query_audit_log(&conn, 2, 0).unwrap();
        assert_eq!(page1.len(), 2);
        let page2 = query_audit_log(&conn, 2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        let page3 = query_audit_log(&conn, 2, 4).unwrap();
        assert_eq!(page3.len(), 1);
    }
}
