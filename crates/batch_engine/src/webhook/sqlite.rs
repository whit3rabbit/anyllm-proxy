// crates/batch_engine/src/webhook/sqlite.rs
//! SQLite-backed webhook delivery queue.

use super::{LeasedDelivery, WebhookDelivery, WebhookQueue};
use crate::db::{format_epoch_iso8601, now_iso8601};
use crate::error::QueueError;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// SQLite-backed webhook delivery queue.
#[derive(Clone)]
pub struct SqliteWebhookQueue {
    db: Arc<Mutex<Connection>>,
}

impl SqliteWebhookQueue {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl WebhookQueue for SqliteWebhookQueue {
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let payload_str = serde_json::to_string(&delivery.payload)
                .map_err(|e| QueueError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO webhook_delivery
                    (delivery_id, event_id, batch_id, url, payload, signing_secret,
                     status, attempts, max_retries, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9)",
                params![
                    delivery.delivery_id,
                    delivery.event_id,
                    delivery.batch_id,
                    delivery.url,
                    payload_str,
                    delivery.signing_secret,
                    delivery.attempts,
                    delivery.max_retries,
                    now_iso8601(),
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let lease_id = format!("whl_{}", uuid::Uuid::new_v4());
            let now = now_iso8601();
            let lease_expires = {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 60;
                format_epoch_iso8601(secs)
            };

            let result = conn.query_row(
                "UPDATE webhook_delivery
                 SET status = 'processing', lease_id = ?1, lease_expires_at = ?2,
                     attempts = attempts + 1
                 WHERE delivery_id = (
                     SELECT delivery_id FROM webhook_delivery
                     WHERE status = 'pending'
                       AND (next_retry_at IS NULL OR next_retry_at <= ?3)
                     ORDER BY created_at ASC
                     LIMIT 1
                 )
                 RETURNING delivery_id, event_id, batch_id, url, payload,
                           signing_secret, attempts, max_retries, next_retry_at",
                params![lease_id, lease_expires, now],
                |row| {
                    let payload_str: String = row.get(4)?;
                    let payload =
                        serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
                    Ok(LeasedDelivery {
                        delivery: WebhookDelivery {
                            delivery_id: row.get(0)?,
                            event_id: row.get(1)?,
                            batch_id: row.get(2)?,
                            url: row.get(3)?,
                            payload,
                            signing_secret: row.get(5)?,
                            attempts: row.get::<_, i64>(6).unwrap_or(0) as u8,
                            max_retries: row.get::<_, i64>(7).unwrap_or(3) as u8,
                            next_retry_at: row.get(8)?,
                        },
                        lease_id: lease_id.clone(),
                    })
                },
            );

            match result {
                Ok(leased) => Ok(Some(leased)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(QueueError::from(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE webhook_delivery SET status = 'delivered', delivered_at = ?1,
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?2",
                params![now_iso8601(), id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn schedule_retry(&self, delivery_id: &str, delay: Duration) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
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
                "UPDATE webhook_delivery SET status = 'pending', next_retry_at = ?1,
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?2",
                params![retry_at, id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError> {
        let db = self.db.clone();
        let id = delivery_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            conn.execute(
                "UPDATE webhook_delivery SET status = 'dead_letter',
                    lease_id = NULL, lease_expires_at = NULL
                 WHERE delivery_id = ?1",
                params![id],
            )?;
            Ok(())
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
                "UPDATE webhook_delivery SET status = 'pending', lease_id = NULL, lease_expires_at = NULL
                 WHERE lease_id IS NOT NULL AND lease_expires_at < ?1 AND status = 'processing'",
                params![now],
            )?;
            Ok(count as u32)
        })
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_batch_engine_tables;

    async fn test_wq() -> SqliteWebhookQueue {
        let conn = Connection::open_in_memory().unwrap();
        init_batch_engine_tables(&conn).unwrap();
        SqliteWebhookQueue::new(Arc::new(Mutex::new(conn)))
    }

    fn make_delivery(id: &str) -> WebhookDelivery {
        WebhookDelivery {
            delivery_id: id.into(),
            event_id: format!("evt_{id}"),
            batch_id: "batch_1".into(),
            url: "https://example.com/webhook".into(),
            payload: serde_json::json!({"type": "batch.completed"}),
            signing_secret: None,
            attempts: 0,
            max_retries: 3,
            next_retry_at: None,
        }
    }

    #[tokio::test]
    async fn enqueue_and_claim() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_1")).await.unwrap();

        let claimed = wq.claim_next().await.unwrap();
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.delivery.delivery_id, "whd_1");

        // Queue is now empty.
        assert!(wq.claim_next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ack_delivery() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_ack")).await.unwrap();
        let claimed = wq.claim_next().await.unwrap().unwrap();
        wq.ack(&claimed.delivery.delivery_id).await.unwrap();

        // Should not be claimable again.
        assert!(wq.claim_next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn retry_and_dead_letter() {
        let wq = test_wq().await;
        wq.enqueue(make_delivery("whd_retry")).await.unwrap();
        let claimed = wq.claim_next().await.unwrap().unwrap();

        // Schedule retry with 0 delay (immediate).
        wq.schedule_retry(&claimed.delivery.delivery_id, Duration::from_secs(0))
            .await
            .unwrap();

        // Should be claimable again.
        let claimed2 = wq.claim_next().await.unwrap();
        assert!(claimed2.is_some());

        // Dead letter.
        wq.dead_letter(&claimed2.unwrap().delivery.delivery_id)
            .await
            .unwrap();
        assert!(wq.claim_next().await.unwrap().is_none());
    }
}
