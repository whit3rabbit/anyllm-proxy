// crates/batch_engine/src/queue/sqlite.rs
//! SQLite-backed JobQueue implementation.

use super::{JobQueue, LeasedItem};
use crate::db::{format_epoch_iso8601, now_iso8601};
use crate::error::QueueError;
use crate::job::*;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// SQLite-backed job queue. Suitable for single-instance deployments.
#[derive(Clone)]
pub struct SqliteQueue {
    db: Arc<Mutex<Connection>>,
}

impl SqliteQueue {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl JobQueue for SqliteQueue {
    async fn enqueue(&self, job: &BatchJob, items: &[BatchItem]) -> Result<(), QueueError> {
        let db = self.db.clone();
        let job = job.clone();
        let items: Vec<BatchItem> = items.to_vec();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let tx = conn.unchecked_transaction()?;

            tx.execute(
                "INSERT INTO batch_job (batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata, total, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    job.id.0,
                    job.status.as_str(),
                    job.execution_mode.as_str(),
                    job.execution_mode.provider(),
                    job.priority,
                    job.key_id,
                    job.input_file_id,
                    job.webhook_url,
                    job.metadata
                        .as_ref()
                        .map(|m| serde_json::to_string(m).unwrap_or_default()),
                    job.request_counts.total,
                    job.created_at,
                    job.expires_at,
                ],
            )?;

            for item in &items {
                let body_str = serde_json::to_string(&item.request.body)
                    .map_err(|e| QueueError::Storage(e.to_string()))?;
                tx.execute(
                    "INSERT INTO batch_item (item_id, batch_id, custom_id, status, model,
                        request_body, source_format, max_retries, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        item.id.0,
                        item.batch_id.0,
                        item.custom_id,
                        item.status.as_str(),
                        item.request.model,
                        body_str,
                        serde_json::to_string(&item.request.source_format)
                            .unwrap_or_else(|_| "\"openai\"".to_string()),
                        item.max_retries,
                        item.created_at,
                    ],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            row_to_job(&conn, &id)
        })
        .await
        .unwrap()
    }

    async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, QueueError> {
        let db = self.db.clone();
        let cursor = cursor.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut sql = String::from(
                "SELECT batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata,
                    total, processing, succeeded, failed, cancelled, expired,
                    created_at, started_at, completed_at, expires_at
                 FROM batch_job WHERE 1=1",
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(kid) = key_id {
                sql.push_str(" AND key_id = ?");
                param_values.push(Box::new(kid));
            }
            if let Some(ref c) = cursor {
                sql.push_str(
                    " AND created_at < (SELECT created_at FROM batch_job WHERE batch_id = ?)",
                );
                param_values.push(Box::new(c.clone()));
            }
            sql.push_str(" ORDER BY created_at DESC LIMIT ?");
            param_values.push(Box::new(limit));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_refs.as_slice(), |row| Ok(batch_job_from_row(row)))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(QueueError::from)
        })
        .await
        .unwrap()
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchJob, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt =
                conn.prepare("SELECT status FROM batch_job WHERE batch_id = ?1")?;
            let status_str: Option<String> = stmt
                .query_row(params![id], |row| row.get(0))
                .ok();

            let Some(status_str) = status_str else {
                return Err(QueueError::NotFound);
            };

            let current = BatchStatus::from_str_status(&status_str);
            let new_status = match current {
                BatchStatus::Queued => BatchStatus::Cancelled,
                BatchStatus::Processing => BatchStatus::Cancelling,
                other if other.is_terminal() => {
                    return row_to_job(&conn, &id)?.ok_or(QueueError::NotFound);
                }
                _ => BatchStatus::Cancelled,
            };

            conn.execute(
                "UPDATE batch_job SET status = ?1, completed_at = CASE WHEN ?1 = 'cancelled' THEN ?2 ELSE completed_at END
                 WHERE batch_id = ?3",
                params![new_status.as_str(), now_iso8601(), id],
            )?;

            // If directly cancelled (was queued), cancel all pending items.
            if new_status == BatchStatus::Cancelled {
                conn.execute(
                    "UPDATE batch_item SET status = 'cancelled' WHERE batch_id = ?1 AND status = 'pending'",
                    params![id],
                )?;
            }

            row_to_job(&conn, &id)?.ok_or(QueueError::NotFound)
        })
        .await
        .unwrap()
    }

    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let lease_id = format!("lease_{}", uuid::Uuid::new_v4());
            let now = now_iso8601();
            // Lease for 120 seconds.
            let lease_expires = {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 120;
                format_epoch_iso8601(secs)
            };

            let result = conn.query_row(
                "UPDATE batch_item
                 SET status = 'processing',
                     lease_id = ?1,
                     lease_expires_at = ?2,
                     attempts = attempts + 1
                 WHERE item_id = (
                     SELECT bi.item_id
                     FROM batch_item bi
                     JOIN batch_job bj ON bi.batch_id = bj.batch_id
                     WHERE bi.status IN ('pending')
                       AND (bi.next_retry_at IS NULL OR bi.next_retry_at <= ?3)
                       AND bj.status IN ('queued', 'processing')
                       AND bj.execution_mode = 'proxy_native'
                     ORDER BY bj.priority DESC, bi.created_at ASC
                     LIMIT 1
                 )
                 RETURNING item_id, batch_id, custom_id, status, model, request_body,
                           source_format, result_status, result_body, attempts,
                           max_retries, last_error, next_retry_at, lease_id,
                           lease_expires_at, idempotency_key, created_at, completed_at",
                params![lease_id, lease_expires, now],
                |row| {
                    let item = batch_item_from_row(row);
                    Ok(LeasedItem {
                        batch_id: item.batch_id.clone(),
                        lease_id: item.lease_id.clone().unwrap_or_default(),
                        lease_expires_at: item.lease_expires_at.clone().unwrap_or_default(),
                        item,
                    })
                },
            );

            match result {
                Ok(leased) => {
                    // Transition parent job to processing if still queued.
                    conn.execute(
                        "UPDATE batch_job SET status = 'processing', started_at = ?1
                         WHERE batch_id = ?2 AND status = 'queued'",
                        params![now, leased.batch_id.0],
                    )?;
                    Ok(Some(leased))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(QueueError::from(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn complete_item(&self, id: &ItemId, result: BatchItemResult) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let result_body =
            serde_json::to_string(&result.body).map_err(|e| QueueError::Storage(e.to_string()))?;
        let status_code = result.status_code;

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'succeeded', result_status = ?1,
                    result_body = ?2, lease_id = NULL, lease_expires_at = NULL,
                    completed_at = ?3
                 WHERE item_id = ?4",
                params![status_code, result_body, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let error = error.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'failed', last_error = ?1,
                    lease_id = NULL, lease_expires_at = NULL, completed_at = ?2
                 WHERE item_id = ?3",
                params![error, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn schedule_retry(
        &self,
        id: &ItemId,
        delay: Duration,
        error: &str,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let error = error.to_string();
        let retry_at = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + delay.as_secs();
            format_epoch_iso8601(secs)
        };

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_item SET status = 'pending', last_error = ?1,
                    next_retry_at = ?2, lease_id = NULL, lease_expires_at = NULL
                 WHERE item_id = ?3",
                params![error, retry_at, id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "INSERT OR IGNORE INTO batch_dead_letter (item_id, batch_id, custom_id, request_body, last_error, attempts, failed_at)
                 SELECT item_id, batch_id, custom_id, request_body, last_error, attempts, ?1
                 FROM batch_item WHERE item_id = ?2",
                params![now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM batch_item
                 WHERE batch_id = ?1 AND status NOT IN ('succeeded', 'failed', 'cancelled')",
                params![id],
                |row| row.get(0),
            )?;
            Ok(count == 0)
        })
        .await
        .unwrap()
    }

    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            // Count final item states.
            let (succeeded, failed, cancelled): (i64, i64, i64) = conn.query_row(
                "SELECT
                    COUNT(CASE WHEN status = 'succeeded' THEN 1 END),
                    COUNT(CASE WHEN status = 'failed' THEN 1 END),
                    COUNT(CASE WHEN status = 'cancelled' THEN 1 END)
                 FROM batch_item WHERE batch_id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;

            conn.execute(
                "UPDATE batch_job SET status = 'completed',
                    succeeded = ?1, failed = ?2, cancelled = ?3,
                    processing = 0, completed_at = ?4
                 WHERE batch_id = ?5",
                params![succeeded, failed, cancelled, now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT batch_id, status, execution_mode, provider, priority,
                    key_id, input_file_id, webhook_url, metadata,
                    total, processing, succeeded, failed, cancelled, expired,
                    created_at, started_at, completed_at, expires_at
                 FROM batch_job
                 WHERE execution_mode = 'native' AND status = 'processing'",
            )?;
            let rows = stmt.query_map([], |row| Ok(batch_job_from_row(row)))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(QueueError::from)
        })
        .await
        .unwrap()
    }

    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let now = now_iso8601();
            let count = conn.execute(
                "UPDATE batch_item SET status = 'pending', lease_id = NULL, lease_expires_at = NULL
                 WHERE lease_id IS NOT NULL AND lease_expires_at < ?1 AND status = 'processing'",
                params![now],
            )?;
            if count > 0 {
                tracing::warn!(count, "reclaimed expired item leases");
            }
            Ok(count as u32)
        })
        .await
        .unwrap()
    }

    async fn update_progress(
        &self,
        id: &BatchId,
        counts: &RequestCounts,
    ) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = id.0.clone();
        let counts = counts.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE batch_job SET
                    processing = ?1, succeeded = ?2, failed = ?3,
                    cancelled = ?4, expired = ?5
                 WHERE batch_id = ?6",
                params![
                    counts.processing,
                    counts.succeeded,
                    counts.failed,
                    counts.cancelled,
                    counts.expired,
                    id,
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_items(&self, batch_id: &BatchId) -> Result<Vec<BatchItem>, QueueError> {
        let db = self.db.clone();
        let batch_id = batch_id.0.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT item_id, batch_id, custom_id, status, model, request_body,
                    source_format, result_status, result_body, attempts,
                    max_retries, last_error, next_retry_at, lease_id,
                    lease_expires_at, idempotency_key, created_at, completed_at
                 FROM batch_item WHERE batch_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![batch_id], |row| Ok(batch_item_from_row(row)))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(QueueError::from)
        })
        .await
        .unwrap()
    }
}

// -- Row mappers --

fn batch_job_from_row(row: &rusqlite::Row) -> BatchJob {
    let status_str: String = row.get(1).unwrap_or_default();
    let exec_mode_str: String = row.get(2).unwrap_or_default();
    let provider: Option<String> = row.get(3).unwrap_or(None);
    let metadata_str: Option<String> = row.get(8).unwrap_or(None);

    let execution_mode = match exec_mode_str.as_str() {
        "native" => ExecutionMode::Native {
            provider: provider.unwrap_or_else(|| "unknown".into()),
        },
        _ => ExecutionMode::ProxyNative,
    };

    BatchJob {
        id: BatchId(row.get(0).unwrap_or_default()),
        status: BatchStatus::from_str_status(&status_str),
        execution_mode,
        priority: row.get::<_, i64>(4).unwrap_or(0) as u8,
        key_id: row.get(5).unwrap_or(None),
        input_file_id: row.get(6).unwrap_or_default(),
        webhook_url: row.get(7).unwrap_or(None),
        metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        request_counts: RequestCounts {
            total: row.get::<_, i64>(9).unwrap_or(0) as u32,
            processing: row.get::<_, i64>(10).unwrap_or(0) as u32,
            succeeded: row.get::<_, i64>(11).unwrap_or(0) as u32,
            failed: row.get::<_, i64>(12).unwrap_or(0) as u32,
            cancelled: row.get::<_, i64>(13).unwrap_or(0) as u32,
            expired: row.get::<_, i64>(14).unwrap_or(0) as u32,
        },
        created_at: row.get(15).unwrap_or_default(),
        started_at: row.get(16).unwrap_or(None),
        completed_at: row.get(17).unwrap_or(None),
        expires_at: row.get(18).unwrap_or_default(),
    }
}

fn batch_item_from_row(row: &rusqlite::Row) -> BatchItem {
    let status_str: String = row.get(3).unwrap_or_default();
    let model: String = row.get(4).unwrap_or_default();
    let body_str: String = row.get(5).unwrap_or_default();
    let source_fmt_str: String = row.get(6).unwrap_or_default();
    let result_status: Option<i64> = row.get(7).unwrap_or(None);
    let result_body_str: Option<String> = row.get(8).unwrap_or(None);

    let source_format =
        serde_json::from_str::<SourceFormat>(&source_fmt_str).unwrap_or(SourceFormat::OpenAI);

    let body = serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

    let result = match (result_status, result_body_str) {
        (Some(code), Some(body_s)) => {
            let body_val = serde_json::from_str(&body_s).unwrap_or(serde_json::Value::Null);
            Some(BatchItemResult {
                status_code: code as u16,
                body: body_val,
            })
        }
        _ => None,
    };

    BatchItem {
        id: ItemId(row.get(0).unwrap_or_default()),
        batch_id: BatchId(row.get(1).unwrap_or_default()),
        custom_id: row.get(2).unwrap_or_default(),
        status: ItemStatus::from_str_status(&status_str),
        request: BatchItemRequest {
            model,
            body,
            source_format,
        },
        result,
        attempts: row.get::<_, i64>(9).unwrap_or(0) as u8,
        max_retries: row.get::<_, i64>(10).unwrap_or(3) as u8,
        last_error: row.get(11).unwrap_or(None),
        next_retry_at: row.get(12).unwrap_or(None),
        lease_id: row.get(13).unwrap_or(None),
        lease_expires_at: row.get(14).unwrap_or(None),
        idempotency_key: row.get(15).unwrap_or(None),
        created_at: row.get(16).unwrap_or_default(),
        completed_at: row.get(17).unwrap_or(None),
    }
}

fn row_to_job(conn: &Connection, batch_id: &str) -> Result<Option<BatchJob>, QueueError> {
    let mut stmt = conn.prepare(
        "SELECT batch_id, status, execution_mode, provider, priority,
            key_id, input_file_id, webhook_url, metadata,
            total, processing, succeeded, failed, cancelled, expired,
            created_at, started_at, completed_at, expires_at
         FROM batch_job WHERE batch_id = ?1",
    )?;
    let mut rows = stmt.query(params![batch_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(batch_job_from_row(row)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_queue() -> SqliteQueue {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        SqliteQueue::new(Arc::new(Mutex::new(conn)))
    }

    fn make_job(id: &str) -> (BatchJob, Vec<BatchItem>) {
        let batch_id = BatchId(id.into());
        let now = crate::db::now_iso8601();
        let job = BatchJob {
            id: batch_id.clone(),
            status: BatchStatus::Queued,
            execution_mode: ExecutionMode::ProxyNative,
            priority: 0,
            key_id: None,
            input_file_id: "file-test".into(),
            webhook_url: None,
            metadata: None,
            request_counts: RequestCounts {
                total: 2,
                ..Default::default()
            },
            created_at: now.clone(),
            started_at: None,
            completed_at: None,
            expires_at: now.clone(),
        };
        let items = vec![
            BatchItem {
                id: ItemId(format!("{id}_item_1")),
                batch_id: batch_id.clone(),
                custom_id: "req-1".into(),
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: "gpt-4o".into(),
                    body: serde_json::json!({"messages": []}),
                    source_format: SourceFormat::OpenAI,
                },
                result: None,
                attempts: 0,
                max_retries: 3,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            },
            BatchItem {
                id: ItemId(format!("{id}_item_2")),
                batch_id: batch_id.clone(),
                custom_id: "req-2".into(),
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: "gpt-4o".into(),
                    body: serde_json::json!({"messages": []}),
                    source_format: SourceFormat::OpenAI,
                },
                result: None,
                attempts: 0,
                max_retries: 3,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            },
        ];
        (job, items)
    }

    #[tokio::test]
    async fn enqueue_and_get() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_test1");
        q.enqueue(&job, &items).await.unwrap();

        let fetched = q.get(&BatchId("batch_test1".into())).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.status, BatchStatus::Queued);
        assert_eq!(fetched.request_counts.total, 2);
    }

    #[tokio::test]
    async fn get_nonexistent() {
        let q = test_queue().await;
        let fetched = q.get(&BatchId("nope".into())).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn cancel_queued_job() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_cancel");
        q.enqueue(&job, &items).await.unwrap();

        let cancelled = q.cancel(&BatchId("batch_cancel".into())).await.unwrap();
        assert_eq!(cancelled.status, BatchStatus::Cancelled);
    }

    #[tokio::test]
    async fn list_with_pagination() {
        let q = test_queue().await;
        for i in 0..5 {
            let (job, items) = make_job(&format!("batch_list_{i}"));
            q.enqueue(&job, &items).await.unwrap();
        }

        let all = q.list(None, None, 10).await.unwrap();
        assert_eq!(all.len(), 5);

        let page = q.list(None, None, 2).await.unwrap();
        assert_eq!(page.len(), 2);
    }

    #[tokio::test]
    async fn get_items() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_items");
        q.enqueue(&job, &items).await.unwrap();

        let fetched = q.get_items(&BatchId("batch_items".into())).await.unwrap();
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0].custom_id, "req-1");
        assert_eq!(fetched[1].custom_id, "req-2");
    }

    #[tokio::test]
    async fn complete_item_and_batch() {
        let q = test_queue().await;
        let (job, items) = make_job("batch_complete");
        q.enqueue(&job, &items).await.unwrap();

        // Complete both items.
        let result = BatchItemResult {
            status_code: 200,
            body: serde_json::json!({"id": "resp-1"}),
        };
        q.complete_item(&ItemId("batch_complete_item_1".into()), result.clone())
            .await
            .unwrap();
        q.complete_item(&ItemId("batch_complete_item_2".into()), result)
            .await
            .unwrap();

        assert!(q
            .is_batch_complete(&BatchId("batch_complete".into()))
            .await
            .unwrap());

        q.complete_batch(&BatchId("batch_complete".into()))
            .await
            .unwrap();
        let job = q
            .get(&BatchId("batch_complete".into()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, BatchStatus::Completed);
        assert_eq!(job.request_counts.succeeded, 2);
    }
}
