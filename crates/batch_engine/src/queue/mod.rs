// crates/batch_engine/src/queue/mod.rs
//! JobQueue trait and implementations.

pub mod sqlite;

use crate::error::QueueError;
use crate::job::*;
use async_trait::async_trait;
use std::time::Duration;

/// A job that has been claimed by a worker. Holds lease metadata.
#[derive(Debug)]
pub struct LeasedItem {
    pub item: BatchItem,
    pub batch_id: BatchId,
    pub lease_id: String,
    pub lease_expires_at: String,
}

/// Core queue abstraction. All methods are async + Send.
#[async_trait]
pub trait JobQueue: Send + Sync + 'static {
    // -- Job lifecycle --
    async fn enqueue(&self, job: &BatchJob, items: &[BatchItem]) -> Result<(), QueueError>;
    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError>;
    async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, QueueError>;
    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, QueueError>;

    // -- Item-level operations (proxy-native path, Phase 2) --
    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError>;
    async fn complete_item(
        &self,
        id: &ItemId,
        result: BatchItemResult,
    ) -> Result<(), QueueError>;
    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError>;
    async fn schedule_retry(
        &self,
        id: &ItemId,
        delay: Duration,
        error: &str,
    ) -> Result<(), QueueError>;
    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError>;

    // -- Batch completion --
    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError>;
    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError>;

    // -- Native batch support --
    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError>;

    // -- Lease management --
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;

    // -- Progress --
    async fn update_progress(
        &self,
        id: &BatchId,
        counts: &RequestCounts,
    ) -> Result<(), QueueError>;

    // -- Items query --
    async fn get_items(&self, batch_id: &BatchId) -> Result<Vec<BatchItem>, QueueError>;
}
