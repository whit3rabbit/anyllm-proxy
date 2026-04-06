// crates/batch_engine/src/webhook/mod.rs
//! Durable webhook delivery queue and dispatcher.

pub mod dispatcher;
pub mod sqlite;

use crate::error::QueueError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A webhook delivery request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub delivery_id: String,
    pub event_id: String,
    pub batch_id: String,
    /// Target URL. Validated against private IP ranges before the first attempt.
    pub url: String,
    pub payload: serde_json::Value,
    /// HMAC secret for `X-Signature-256` header generation.
    /// Skipped during serialization so it is never written to the SQLite queue.
    #[serde(skip)]
    pub signing_secret: Option<String>,
    pub attempts: u8,
    pub max_retries: u8,
    /// ISO 8601 UTC timestamp for the next retry. `None` on the first attempt.
    pub next_retry_at: Option<String>,
}

/// A claimed webhook delivery with lease info.
#[derive(Debug)]
pub struct LeasedDelivery {
    pub delivery: WebhookDelivery,
    pub lease_id: String,
}

/// Durable webhook delivery queue. Persists deliveries in SQLite so they
/// survive process restarts and can be retried by the dispatcher background task.
#[async_trait]
pub trait WebhookQueue: Send + Sync + 'static {
    /// Persist a new delivery request. The dispatcher picks it up asynchronously.
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError>;
    /// Claim the next pending delivery with a short-lived lease for the dispatcher.
    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError>;
    /// Acknowledge successful delivery (removes from queue).
    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError>;
    /// Schedule a retry after `delay` (increments attempt count).
    async fn schedule_retry(&self, delivery_id: &str, delay: Duration) -> Result<(), QueueError>;
    /// Move to dead-letter state after max retries (no further attempts).
    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError>;
    /// Re-enqueue deliveries whose dispatcher leases have expired.
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;
}
