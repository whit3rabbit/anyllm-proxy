// SQLite CRUD for batch_file and batch_job tables.

use super::{BatchJob, BatchStatus, RequestCounts};
use crate::admin::db::now_iso8601;
use rusqlite::{params, Connection};

/// Mapping from our Anthropic batch ID to the upstream OpenAI batch ID.
pub struct AnthropicBatchMap {
    pub our_batch_id: String,
    pub openai_batch_id: String,
    pub openai_output_file_id: Option<String>,
    pub model: String,
}

/// Create the anthropic_batch_map table if it doesn't exist.
pub fn init_anthropic_batch_map_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS anthropic_batch_map (
            our_batch_id TEXT PRIMARY KEY,
            openai_batch_id TEXT NOT NULL,
            openai_output_file_id TEXT,
            model TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );",
    )
}

/// Store an Anthropic->OpenAI batch id mapping.
pub fn insert_anthropic_batch_map(
    conn: &Connection,
    our_batch_id: &str,
    openai_batch_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO anthropic_batch_map (our_batch_id, openai_batch_id) VALUES (?1, ?2)",
        params![our_batch_id, openai_batch_id],
    )?;
    Ok(())
}

/// Look up a mapping by our batch ID.
pub fn get_anthropic_batch_map(
    conn: &Connection,
    our_batch_id: &str,
) -> rusqlite::Result<Option<AnthropicBatchMap>> {
    let mut stmt = conn.prepare(
        "SELECT our_batch_id, openai_batch_id, openai_output_file_id, model
         FROM anthropic_batch_map WHERE our_batch_id = ?1",
    )?;
    let mut rows = stmt.query(params![our_batch_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(AnthropicBatchMap {
            our_batch_id: row.get(0)?,
            openai_batch_id: row.get(1)?,
            openai_output_file_id: row.get(2)?,
            model: row.get(3)?,
        }))
    } else {
        Ok(None)
    }
}

/// Update the output_file_id once the batch completes.
pub fn set_anthropic_batch_output_file(
    conn: &Connection,
    our_batch_id: &str,
    output_file_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE anthropic_batch_map SET openai_output_file_id = ?1 WHERE our_batch_id = ?2",
        params![output_file_id, our_batch_id],
    )?;
    Ok(())
}

/// Create the batch_file and batch_job tables if they do not exist.
pub fn init_batch_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
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
    // Also create the Anthropic batch ID mapping table.
    init_anthropic_batch_map_table(conn)?;
    Ok(())
}

/// Insert a new batch file record.
#[allow(clippy::too_many_arguments)]
pub fn insert_batch_file(
    conn: &Connection,
    file_id: &str,
    key_id: Option<i64>,
    purpose: &str,
    filename: Option<&str>,
    byte_size: i64,
    line_count: i64,
    content: &[u8],
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO batch_file (file_id, key_id, purpose, filename, byte_size, line_count, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            file_id,
            key_id,
            purpose,
            filename,
            byte_size,
            line_count,
            content,
            now_iso8601(),
        ],
    )?;
    Ok(())
}

/// Check if a batch file exists by file_id. Returns (byte_size, line_count, created_at) if found.
pub fn get_batch_file_meta(
    conn: &Connection,
    file_id: &str,
) -> rusqlite::Result<Option<(i64, i64, String)>> {
    let mut stmt = conn
        .prepare("SELECT byte_size, line_count, created_at FROM batch_file WHERE file_id = ?1")?;
    let mut rows = stmt.query_map(params![file_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.next().transpose()
}

/// Insert a new batch job record.
pub fn insert_batch_job(
    conn: &Connection,
    batch_id: &str,
    key_id: Option<i64>,
    input_file_id: &str,
    backend_name: &str,
    line_count: i64,
    metadata: Option<&serde_json::Value>,
) -> rusqlite::Result<()> {
    let meta_str = metadata.map(|m| serde_json::to_string(m).unwrap_or_default());
    conn.execute(
        "INSERT INTO batch_job (batch_id, key_id, input_file_id, backend_name, status, request_counts_total, created_at, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            batch_id,
            key_id,
            input_file_id,
            backend_name,
            BatchStatus::Validating.as_str(),
            line_count,
            now_iso8601(),
            meta_str,
        ],
    )?;
    Ok(())
}

/// Fetch a single batch job by batch_id.
pub fn get_batch_job(conn: &Connection, batch_id: &str) -> rusqlite::Result<Option<BatchJob>> {
    let mut stmt = conn.prepare(
        "SELECT batch_id, input_file_id, backend_name, status,
                request_counts_total, request_counts_completed, request_counts_failed,
                output_file_id, error_file_id, created_at, completed_at, expires_at, metadata
         FROM batch_job WHERE batch_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![batch_id], row_to_batch_job)?;
    rows.next().transpose()
}

/// Update the status (and optional completion fields) of a batch job.
pub fn update_batch_job_status(
    conn: &Connection,
    batch_id: &str,
    status: &BatchStatus,
    completed_count: Option<i64>,
    failed_count: Option<i64>,
    output_file_id: Option<&str>,
    error_file_id: Option<&str>,
) -> rusqlite::Result<bool> {
    let completed_at = if matches!(status, BatchStatus::Completed | BatchStatus::Failed) {
        Some(now_iso8601())
    } else {
        None
    };

    let changed = conn.execute(
        "UPDATE batch_job SET status = ?1,
            request_counts_completed = COALESCE(?2, request_counts_completed),
            request_counts_failed = COALESCE(?3, request_counts_failed),
            output_file_id = COALESCE(?4, output_file_id),
            error_file_id = COALESCE(?5, error_file_id),
            completed_at = COALESCE(?6, completed_at)
         WHERE batch_id = ?7",
        params![
            status.as_str(),
            completed_count,
            failed_count,
            output_file_id,
            error_file_id,
            completed_at,
            batch_id,
        ],
    )?;
    Ok(changed > 0)
}

/// List batch jobs, optionally filtered by key_id, with cursor pagination.
pub fn list_batch_jobs(
    conn: &Connection,
    key_id: Option<i64>,
    limit: u32,
    after: Option<&str>,
) -> rusqlite::Result<Vec<BatchJob>> {
    let mut sql = String::from(
        "SELECT batch_id, input_file_id, backend_name, status,
                request_counts_total, request_counts_completed, request_counts_failed,
                output_file_id, error_file_id, created_at, completed_at, expires_at, metadata
         FROM batch_job WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(kid) = key_id {
        sql.push_str(" AND key_id = ?");
        param_values.push(Box::new(kid));
    }
    if let Some(cursor) = after {
        sql.push_str(" AND batch_id < ?");
        param_values.push(Box::new(cursor.to_string()));
    }

    sql.push_str(" ORDER BY id DESC LIMIT ?");
    param_values.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_batch_job)?;
    rows.collect()
}

/// Map a SQLite row to a BatchJob.
fn row_to_batch_job(row: &rusqlite::Row) -> rusqlite::Result<BatchJob> {
    let status_str: String = row.get(3)?;
    let created_at_str: String = row.get(9)?;
    let metadata_str: Option<String> = row.get(12)?;

    Ok(BatchJob {
        id: row.get(0)?,
        object: "batch".to_string(),
        endpoint: "/v1/chat/completions".to_string(),
        status: BatchStatus::from_str_status(&status_str),
        input_file_id: row.get(1)?,
        completion_window: "24h".to_string(),
        created_at: iso8601_to_epoch(&created_at_str),
        request_counts: RequestCounts {
            total: row.get(4)?,
            completed: row.get(5)?,
            failed: row.get(6)?,
        },
        metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        output_file_id: row.get(7)?,
        error_file_id: row.get(8)?,
        completed_at: row
            .get::<_, Option<String>>(10)?
            .map(|s| iso8601_to_epoch(&s)),
        expires_at: row
            .get::<_, Option<String>>(11)?
            .map(|s| iso8601_to_epoch(&s)),
    })
}

/// Approximate conversion from ISO 8601 string to unix epoch seconds.
/// Falls back to 0 on parse failure (non-critical metadata field).
fn iso8601_to_epoch(s: &str) -> i64 {
    // Parse "YYYY-MM-DDTHH:MM:SSZ" manually (no chrono dependency).
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return 0;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_str = parts[1].trim_end_matches('Z');
    let time_parts: Vec<u64> = time_str.split(':').filter_map(|p| p.parse().ok()).collect();

    if date_parts.len() != 3 || time_parts.len() != 3 {
        return 0;
    }

    let (y, m, d) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hh, mm, ss) = (time_parts[0], time_parts[1], time_parts[2]);

    // Days from epoch using the inverse of the Howard Hinnant algorithm.
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj / 400;
    let yoe = y_adj - era * 400;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    (days * 86400 + hh * 3600 + mm * 60 + ss) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::db::init_db;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_batch_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_get_batch_file() {
        let conn = test_db();
        insert_batch_file(
            &conn,
            "file-abc123",
            None,
            "batch",
            Some("test.jsonl"),
            1024,
            10,
            b"test content",
        )
        .unwrap();

        let meta = get_batch_file_meta(&conn, "file-abc123").unwrap();
        assert!(meta.is_some());
        let (size, count, _created) = meta.unwrap();
        assert_eq!(size, 1024);
        assert_eq!(count, 10);
    }

    #[test]
    fn insert_and_get_batch_job() {
        let conn = test_db();
        insert_batch_file(&conn, "file-input1", None, "batch", None, 512, 5, b"data").unwrap();

        insert_batch_job(&conn, "batch-job1", None, "file-input1", "openai", 5, None).unwrap();

        let job = get_batch_job(&conn, "batch-job1").unwrap();
        assert!(job.is_some());
        let job = job.unwrap();
        assert_eq!(job.id, "batch-job1");
        assert_eq!(job.status, BatchStatus::Validating);
        assert_eq!(job.request_counts.total, 5);
        assert_eq!(job.input_file_id, "file-input1");
    }

    #[test]
    fn update_batch_job_status_works() {
        let conn = test_db();
        insert_batch_file(&conn, "file-u1", None, "batch", None, 100, 2, b"d").unwrap();
        insert_batch_job(&conn, "batch-u1", None, "file-u1", "openai", 2, None).unwrap();

        let ok = update_batch_job_status(
            &conn,
            "batch-u1",
            &BatchStatus::Completed,
            Some(2),
            Some(0),
            Some("file-out1"),
            None,
        )
        .unwrap();
        assert!(ok);

        let job = get_batch_job(&conn, "batch-u1").unwrap().unwrap();
        assert_eq!(job.status, BatchStatus::Completed);
        assert_eq!(job.request_counts.completed, 2);
        assert!(job.output_file_id.is_some());
        assert!(job.completed_at.is_some());
    }

    #[test]
    fn list_batch_jobs_with_pagination() {
        let conn = test_db();
        insert_batch_file(&conn, "file-l1", None, "batch", None, 10, 1, b"d").unwrap();

        for i in 0..5 {
            insert_batch_job(
                &conn,
                &format!("batch-l{i}"),
                Some(1),
                "file-l1",
                "openai",
                1,
                None,
            )
            .unwrap();
        }

        let all = list_batch_jobs(&conn, Some(1), 10, None).unwrap();
        assert_eq!(all.len(), 5);

        let page = list_batch_jobs(&conn, Some(1), 2, None).unwrap();
        assert_eq!(page.len(), 2);

        // Different key_id returns nothing
        let empty = list_batch_jobs(&conn, Some(999), 10, None).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn get_nonexistent_job() {
        let conn = test_db();
        let job = get_batch_job(&conn, "batch-nope").unwrap();
        assert!(job.is_none());
    }

    #[test]
    fn iso8601_round_trip() {
        // 2026-03-22T10:30:00Z
        let epoch = iso8601_to_epoch("2026-03-22T10:30:00Z");
        assert!(epoch > 0);
    }

    #[test]
    fn anthropic_batch_map_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_batch_tables(&conn).unwrap();
        init_anthropic_batch_map_table(&conn).unwrap();

        insert_anthropic_batch_map(&conn, "msgbatch_our1", "batch_openai1").unwrap();
        let record = get_anthropic_batch_map(&conn, "msgbatch_our1")
            .unwrap()
            .unwrap();
        assert_eq!(record.openai_batch_id, "batch_openai1");
        assert!(record.openai_output_file_id.is_none());

        set_anthropic_batch_output_file(&conn, "msgbatch_our1", "file-output1").unwrap();
        let record2 = get_anthropic_batch_map(&conn, "msgbatch_our1")
            .unwrap()
            .unwrap();
        assert_eq!(
            record2.openai_output_file_id.as_deref(),
            Some("file-output1")
        );
    }
}
