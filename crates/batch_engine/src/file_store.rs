// crates/batch_engine/src/file_store.rs
//! Batch file storage (upload, metadata, content retrieval).

use crate::db::now_iso8601;
use crate::error::QueueError;
use rusqlite::{params, Connection};
use std::sync::Arc;
// std::sync::Mutex is the correct primitive here: rusqlite is synchronous,
// and these locks are only acquired inside spawn_blocking (never across .await).
use std::sync::Mutex;

/// Batch file metadata (without content blob).
#[derive(Debug, Clone)]
pub struct BatchFileMeta {
    pub file_id: String,
    pub key_id: Option<i64>,
    pub byte_size: i64,
    pub line_count: i64,
    pub filename: Option<String>,
    pub created_at: String,
}

/// Manages batch file storage in SQLite.
#[derive(Clone)]
pub struct FileStore {
    db: Arc<Mutex<Connection>>,
}

impl FileStore {
    /// Create a new file store backed by the given SQLite connection.
    ///
    /// The connection is shared with the job queue and webhook queue via `Arc<Mutex>`.
    /// All operations run on the blocking thread pool via `spawn_blocking` to avoid
    /// holding the mutex across `.await` points.
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Store a batch file. Returns the file_id.
    pub async fn insert(
        &self,
        file_id: &str,
        key_id: Option<i64>,
        filename: Option<&str>,
        content: &[u8],
        line_count: i64,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();
        let filename = filename.map(|s| s.to_string());
        let content = content.to_vec();
        let byte_size = content.len() as i64;

        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO batch_file (file_id, key_id, purpose, filename, byte_size, line_count, content, created_at)
                 VALUES (?1, ?2, 'batch', ?3, ?4, ?5, ?6, ?7)",
                params![file_id, key_id, filename, byte_size, line_count, content, now_iso8601()],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    /// Get file metadata (without content).
    pub async fn get_meta(&self, file_id: &str) -> Result<Option<BatchFileMeta>, QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT file_id, key_id, byte_size, line_count, filename, created_at FROM batch_file WHERE file_id = ?1",
            )?;
            let mut rows = stmt.query(params![file_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(BatchFileMeta {
                    file_id: row.get(0)?,
                    key_id: row.get(1)?,
                    byte_size: row.get(2)?,
                    line_count: row.get(3)?,
                    filename: row.get(4)?,
                    created_at: row.get(5)?,
                }))
            } else {
                Ok(None)
            }
        })
        .await
        .unwrap()
    }

    /// Get file content (raw JSONL bytes).
    pub async fn get_content(&self, file_id: &str) -> Result<Option<Vec<u8>>, QueueError> {
        let db = self.db.clone();
        let file_id = file_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap();
            let mut stmt = conn.prepare("SELECT content FROM batch_file WHERE file_id = ?1")?;
            let mut rows = stmt.query(params![file_id])?;
            if let Some(row) = rows.next()? {
                let content: Vec<u8> = row.get(0)?;
                Ok(Some(content))
            } else {
                Ok(None)
            }
        })
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_store() -> FileStore {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        FileStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn insert_and_get_meta() {
        let store = test_store().await;
        store
            .insert("file-abc", None, Some("test.jsonl"), b"line1\nline2", 2)
            .await
            .unwrap();

        let meta = store.get_meta("file-abc").await.unwrap().unwrap();
        assert_eq!(meta.file_id, "file-abc");
        assert_eq!(meta.key_id, None);
        assert_eq!(meta.byte_size, 11);
        assert_eq!(meta.line_count, 2);
        assert_eq!(meta.filename.as_deref(), Some("test.jsonl"));
    }

    #[tokio::test]
    async fn get_meta_returns_key_id() {
        let store = test_store().await;
        store
            .insert("file-owned", Some(42), None, b"line1", 1)
            .await
            .unwrap();

        let meta = store.get_meta("file-owned").await.unwrap().unwrap();
        assert_eq!(meta.key_id, Some(42));
    }

    #[tokio::test]
    async fn get_content_roundtrip() {
        let store = test_store().await;
        let data = b"test content bytes";
        store.insert("file-xyz", None, None, data, 1).await.unwrap();

        let content = store.get_content("file-xyz").await.unwrap().unwrap();
        assert_eq!(content, data);
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = test_store().await;
        assert!(store.get_meta("file-nope").await.unwrap().is_none());
        assert!(store.get_content("file-nope").await.unwrap().is_none());
    }
}
