use super::common::chrono_now;
use rusqlite::{params, Connection};

/// Star a provider. Idempotent: re-favoriting an already-favorite provider is a no-op.
pub fn insert_favorite(conn: &Connection, provider_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO provider_favorites (provider_id, created_at) VALUES (?1, ?2)",
        params![provider_id, chrono_now()],
    )?;
    Ok(())
}

/// Unstar a provider. Returns true if a row was removed.
pub fn delete_favorite(conn: &Connection, provider_id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "DELETE FROM provider_favorites WHERE provider_id = ?1",
        [provider_id],
    )?;
    Ok(n > 0)
}

/// List favorite provider ids in the order they were starred.
///
/// `created_at` is second-precision, so favorites starred within the same second
/// would otherwise tie with undefined order; `rowid` (monotonic on insert) breaks
/// the tie deterministically into true insertion order.
pub fn list_favorites(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT provider_id FROM provider_favorites ORDER BY created_at, rowid")?;
    let rows: rusqlite::Result<Vec<String>> = stmt.query_map([], |r| r.get(0))?.collect();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::db::init_db;

    #[test]
    fn favorites_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(list_favorites(&conn).unwrap().is_empty());

        insert_favorite(&conn, "ollama").unwrap();
        insert_favorite(&conn, "groq").unwrap();
        insert_favorite(&conn, "ollama").unwrap(); // idempotent
        assert_eq!(list_favorites(&conn).unwrap(), vec!["ollama", "groq"]);

        assert!(delete_favorite(&conn, "ollama").unwrap());
        assert!(!delete_favorite(&conn, "ollama").unwrap()); // already gone
        assert_eq!(list_favorites(&conn).unwrap(), vec!["groq"]);
    }
}
