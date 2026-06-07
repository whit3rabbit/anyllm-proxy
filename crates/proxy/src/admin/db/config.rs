use super::common::chrono_now;
use rusqlite::{params, Connection};

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

/// Upsert a batch of env-file key-value pairs into the `env_import` table.
/// Existing keys are overwritten; `imported_at` is set to the current UTC timestamp.
/// All rows are written in a single transaction for atomicity and performance.
pub fn upsert_env_import(
    conn: &mut Connection,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    let now = chrono_now();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO env_import (key, value, imported_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, imported_at = excluded.imported_at",
        )?;
        for (key, value) in pairs {
            stmt.execute(rusqlite::params![key, value, now])?;
        }
    }
    tx.commit()
}

/// Return all rows from `env_import` as (key, value) pairs.
/// Used during startup to apply persisted imports before the async runtime starts.
pub fn list_env_import(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT key, value FROM env_import ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::init_db(&conn).unwrap();
        conn
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
}
