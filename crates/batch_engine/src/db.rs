// crates/batch_engine/src/db.rs
//! SQLite schema initialization for batch_engine tables.

use rusqlite::Connection;

/// ISO 8601 timestamp for "now" in UTC.
pub fn now_iso8601() -> String {
    // Replicates the pattern used in proxy's admin/db.rs.
    // Using SystemTime to avoid chrono dependency.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Convert epoch seconds to ISO 8601. Simplified: just store epoch
    // and use SQLite's datetime() for display. But for compatibility
    // with existing code, produce a formatted string.
    let secs = now;
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Civil date from days since epoch (Howard Hinnant algorithm).
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_val = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_val = if m_val <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y_val, m_val, d, h, m, s
    )
}

/// Initialize all batch_engine tables.
pub fn init_batch_engine_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS batch_job (
            batch_id          TEXT PRIMARY KEY,
            status            TEXT NOT NULL DEFAULT 'queued',
            execution_mode    TEXT NOT NULL,
            provider          TEXT,
            provider_batch_id TEXT,
            priority          INTEGER NOT NULL DEFAULT 0,
            key_id            INTEGER,
            input_file_id     TEXT NOT NULL,
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

        CREATE TABLE IF NOT EXISTS batch_dead_letter (
            item_id      TEXT PRIMARY KEY,
            batch_id     TEXT NOT NULL,
            custom_id    TEXT NOT NULL,
            request_body TEXT NOT NULL,
            last_error   TEXT,
            attempts     INTEGER NOT NULL,
            failed_at    TEXT NOT NULL
        );

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

        CREATE TABLE IF NOT EXISTS anthropic_batch_map (
            our_batch_id    TEXT PRIMARY KEY,
            engine_batch_id TEXT NOT NULL,
            model           TEXT NOT NULL,
            created_at      TEXT NOT NULL
        );

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
        ",
    )
}

/// Migrate old batch tables (from proxy's admin/db.rs schema) if they exist.
/// Renames them to _v1 suffix. Safe to call multiple times.
pub fn migrate_old_tables(conn: &Connection) -> rusqlite::Result<()> {
    // Check if old-schema batch_job exists (has `backend_name` column).
    let has_old_batch_job: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('batch_job') WHERE name = 'backend_name'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if has_old_batch_job {
        conn.execute_batch(
            "ALTER TABLE batch_job RENAME TO batch_job_v1;
             ALTER TABLE batch_file RENAME TO batch_file_v1;",
        )?;
        tracing::info!("migrated old batch_job and batch_file tables to _v1");
    }

    // Migrate old anthropic_batch_map if it has openai_batch_id column.
    let has_old_abm: bool = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('anthropic_batch_map') WHERE name = 'openai_batch_id'",
        )
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if has_old_abm {
        conn.execute_batch("ALTER TABLE anthropic_batch_map RENAME TO anthropic_batch_map_v1;")?;
        tracing::info!("migrated old anthropic_batch_map to _v1");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_tables_succeeds() {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();

        // Verify tables exist by querying them.
        let tables = [
            "batch_job",
            "batch_item",
            "batch_file",
            "webhook_delivery",
            "batch_event_log",
        ];
        for table in tables {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "expected empty table {table}");
        }
    }

    #[test]
    fn init_tables_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        init_batch_engine_tables(&conn).unwrap(); // no error
    }

    #[test]
    fn now_iso8601_format() {
        let ts = now_iso8601();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
    }
}
