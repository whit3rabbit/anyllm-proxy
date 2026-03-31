// Anthropic-to-OpenAI batch ID mapping table.
// The batch_file and batch_job tables are now managed by anyllm_batch_engine.

use rusqlite::{params, Connection};

/// Mapping from our Anthropic batch ID to the upstream OpenAI batch ID.
pub struct AnthropicBatchMap {
    pub our_batch_id: String,
    pub openai_batch_id: String,
    pub openai_output_file_id: Option<String>,
    pub model: String,
}

/// Create the anthropic_batch_map table if it doesn't exist (old schema for proxy use).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_batch_map_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
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
