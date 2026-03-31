// crates/batch_engine/src/engine.rs
//! BatchEngine: the main entry point for batch operations.
//! Thin facade over JobQueue, FileStore, and WebhookQueue.

use crate::db::now_iso8601;
use crate::error::EngineError;
use crate::file_store::FileStore;
use crate::job::*;
use crate::queue::JobQueue;
use crate::webhook::{WebhookDelivery, WebhookQueue};
use std::sync::Arc;

/// The main batch engine. Holds references to queue, file store, and webhook queue.
pub struct BatchEngine<Q: JobQueue, W: WebhookQueue> {
    pub queue: Arc<Q>,
    pub file_store: FileStore,
    pub webhook_queue: Arc<W>,
    pub global_webhook_urls: Vec<String>,
    pub webhook_signing_secret: Option<String>,
}

impl<Q: JobQueue, W: WebhookQueue> BatchEngine<Q, W> {
    pub async fn submit(&self, submission: BatchSubmission) -> Result<BatchJob, EngineError> {
        self.file_store
            .get_meta(&submission.input_file_id)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?
            .ok_or_else(|| EngineError::FileNotFound(submission.input_file_id.clone()))?;

        const DEFAULT_MAX_RETRIES: u8 = 3;

        let now = now_iso8601();
        let batch_id = BatchId::new();
        let total = submission.items.len() as u32;

        let job = BatchJob {
            id: batch_id.clone(),
            status: BatchStatus::Queued,
            execution_mode: submission.execution_mode.clone(),
            priority: submission.priority,
            key_id: submission.key_id,
            input_file_id: submission.input_file_id,
            webhook_url: submission.webhook_url.clone(),
            metadata: submission.metadata,
            request_counts: RequestCounts {
                total,
                ..Default::default()
            },
            created_at: now.clone(),
            started_at: None,
            completed_at: None,
            expires_at: now.clone(), // TODO: add 24h
        };

        let items: Vec<BatchItem> = submission
            .items
            .into_iter()
            .map(|si| BatchItem {
                id: ItemId::new(),
                batch_id: batch_id.clone(),
                custom_id: si.custom_id,
                status: ItemStatus::Pending,
                request: BatchItemRequest {
                    model: si.model,
                    body: si.body,
                    source_format: si.source_format,
                },
                result: None,
                attempts: 0,
                max_retries: DEFAULT_MAX_RETRIES,
                last_error: None,
                next_retry_at: None,
                lease_id: None,
                lease_expires_at: None,
                idempotency_key: None,
                created_at: now.clone(),
                completed_at: None,
            })
            .collect();

        self.queue
            .enqueue(&job, &items)
            .await
            .map_err(EngineError::Queue)?;

        self.fire_webhook(
            &batch_id,
            "batch.queued",
            serde_json::json!({
                "batch_id": batch_id.0,
                "total_items": total,
                "execution_mode": job.execution_mode.as_str(),
            }),
            None,
        )
        .await;

        Ok(job)
    }

    /// Get a batch job by ID.
    pub async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, EngineError> {
        self.queue.get(id).await.map_err(EngineError::Queue)
    }

    /// List batch jobs.
    pub async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, EngineError> {
        self.queue
            .list(key_id, cursor, limit)
            .await
            .map_err(EngineError::Queue)
    }

    /// Cancel a batch job.
    pub async fn cancel(&self, id: &BatchId) -> Result<BatchJob, EngineError> {
        let job = self.queue.cancel(id).await.map_err(EngineError::Queue)?;

        if job.status == BatchStatus::Cancelled {
            self.fire_webhook(
                id,
                "batch.cancelled",
                serde_json::json!({ "batch_id": id.0 }),
                job.webhook_url.as_deref(),
            )
            .await;
        }

        Ok(job)
    }

    /// Get items for a batch (used for result retrieval).
    pub async fn get_items(&self, id: &BatchId) -> Result<Vec<BatchItem>, EngineError> {
        self.queue.get_items(id).await.map_err(EngineError::Queue)
    }

    /// Fire a webhook to all configured URLs.
    /// `batch_webhook_url`: per-batch URL for terminal events; callers pass it from the job
    /// they already hold to avoid an extra database round-trip.
    async fn fire_webhook(
        &self,
        batch_id: &BatchId,
        event_type: &str,
        payload: serde_json::Value,
        batch_webhook_url: Option<&str>,
    ) {
        const DEFAULT_MAX_RETRIES: u8 = 3;

        let event_id = format!("evt_{}", uuid::Uuid::new_v4());

        let mut urls: Vec<(String, Option<String>)> = self
            .global_webhook_urls
            .iter()
            .map(|u| (u.clone(), self.webhook_signing_secret.clone()))
            .collect();

        if let Some(url) = batch_webhook_url {
            urls.push((url.to_string(), self.webhook_signing_secret.clone()));
        }

        let full_payload = serde_json::json!({
            "event_id": event_id,
            "event_type": event_type,
            "data": payload,
        });

        for (url, secret) in urls {
            let delivery = WebhookDelivery {
                delivery_id: format!("whd_{}", uuid::Uuid::new_v4()),
                event_id: event_id.clone(),
                batch_id: batch_id.0.clone(),
                url,
                payload: full_payload.clone(),
                signing_secret: secret,
                attempts: 0,
                max_retries: DEFAULT_MAX_RETRIES,
                next_retry_at: None,
            };
            if let Err(e) = self.webhook_queue.enqueue(delivery).await {
                tracing::error!(error = %e, "failed to enqueue webhook delivery");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;
    use crate::file_store::FileStore;
    use crate::queue::sqlite::SqliteQueue;
    use crate::webhook::sqlite::SqliteWebhookQueue;
    use rusqlite::Connection;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn test_engine() -> BatchEngine<SqliteQueue, SqliteWebhookQueue> {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));

        BatchEngine {
            queue: Arc::new(SqliteQueue::new(db.clone())),
            file_store: FileStore::new(db.clone()),
            webhook_queue: Arc::new(SqliteWebhookQueue::new(db)),
            global_webhook_urls: vec![],
            webhook_signing_secret: None,
        }
    }

    #[tokio::test]
    async fn submit_and_get() {
        let engine = test_engine().await;

        // Upload a file first.
        engine
            .file_store
            .insert("file-sub1", None, None, b"test", 2)
            .await
            .unwrap();

        let job = engine
            .submit(BatchSubmission {
                items: vec![
                    SubmissionItem {
                        custom_id: "req-1".into(),
                        model: "gpt-4o".into(),
                        body: serde_json::json!({}),
                        source_format: SourceFormat::OpenAI,
                    },
                    SubmissionItem {
                        custom_id: "req-2".into(),
                        model: "gpt-4o".into(),
                        body: serde_json::json!({}),
                        source_format: SourceFormat::OpenAI,
                    },
                ],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-sub1".into(),
                key_id: Some(42),
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await
            .unwrap();

        assert_eq!(job.status, BatchStatus::Queued);
        assert_eq!(job.request_counts.total, 2);
        assert_eq!(job.key_id, Some(42));

        let fetched = engine.get(&job.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, job.id);
    }

    #[tokio::test]
    async fn submit_missing_file() {
        let engine = test_engine().await;
        let result = engine
            .submit(BatchSubmission {
                items: vec![],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-nope".into(),
                key_id: None,
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancel_job() {
        let engine = test_engine().await;
        engine
            .file_store
            .insert("file-cancel", None, None, b"test", 1)
            .await
            .unwrap();

        let job = engine
            .submit(BatchSubmission {
                items: vec![SubmissionItem {
                    custom_id: "r1".into(),
                    model: "gpt-4o".into(),
                    body: serde_json::json!({}),
                    source_format: SourceFormat::OpenAI,
                }],
                execution_mode: ExecutionMode::ProxyNative,
                input_file_id: "file-cancel".into(),
                key_id: None,
                webhook_url: None,
                metadata: None,
                priority: 0,
            })
            .await
            .unwrap();

        let cancelled = engine.cancel(&job.id).await.unwrap();
        assert_eq!(cancelled.status, BatchStatus::Cancelled);
    }
}
