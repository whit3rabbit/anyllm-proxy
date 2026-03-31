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
    pub url: String,
    pub payload: serde_json::Value,
    #[serde(skip)]
    pub signing_secret: Option<String>,
    pub attempts: u8,
    pub max_retries: u8,
    pub next_retry_at: Option<String>,
}

/// A claimed webhook delivery with lease info.
#[derive(Debug)]
pub struct LeasedDelivery {
    pub delivery: WebhookDelivery,
    pub lease_id: String,
}

/// Durable webhook delivery queue.
#[async_trait]
pub trait WebhookQueue: Send + Sync + 'static {
    async fn enqueue(&self, delivery: WebhookDelivery) -> Result<(), QueueError>;
    async fn claim_next(&self) -> Result<Option<LeasedDelivery>, QueueError>;
    async fn ack(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn schedule_retry(
        &self,
        delivery_id: &str,
        delay: Duration,
    ) -> Result<(), QueueError>;
    async fn dead_letter(&self, delivery_id: &str) -> Result<(), QueueError>;
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;
}
