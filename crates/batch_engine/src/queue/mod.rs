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

    /// Atomically write a new job and all its items to the queue.
    async fn enqueue(&self, job: &BatchJob, items: &[BatchItem]) -> Result<(), QueueError>;
    /// Fetch a job by ID. Returns `None` if not found.
    async fn get(&self, id: &BatchId) -> Result<Option<BatchJob>, QueueError>;
    /// List jobs, optionally filtered by `key_id`, using cursor-based pagination.
    async fn list(
        &self,
        key_id: Option<i64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BatchJob>, QueueError>;
    /// Transition a job to `Cancelling` or `Cancelled` and return the updated job.
    async fn cancel(&self, id: &BatchId) -> Result<BatchJob, QueueError>;

    // -- Item-level operations (proxy-native path) --

    /// Claim the next pending item with a short-lived lease. Returns `None` when
    /// the queue is empty. Workers must either `complete_item`, `fail_item`,
    /// `schedule_retry`, or `dead_letter` before the lease expires.
    async fn claim_next_item(&self) -> Result<Option<LeasedItem>, QueueError>;
    /// Mark an item as successfully completed with its result payload.
    async fn complete_item(&self, id: &ItemId, result: BatchItemResult) -> Result<(), QueueError>;
    /// Mark an item as permanently failed (max retries exhausted or unrecoverable error).
    async fn fail_item(&self, id: &ItemId, error: &str) -> Result<(), QueueError>;
    /// Schedule an item to be retried after `delay`.
    async fn schedule_retry(
        &self,
        id: &ItemId,
        delay: Duration,
        error: &str,
    ) -> Result<(), QueueError>;
    /// Move an item to the dead-letter state (no further retries).
    async fn dead_letter(&self, id: &ItemId) -> Result<(), QueueError>;

    // -- Batch completion --

    /// Returns true when all items in the batch are in a terminal state.
    async fn is_batch_complete(&self, id: &BatchId) -> Result<bool, QueueError>;
    /// Transition the batch job itself to `Completed`.
    async fn complete_batch(&self, id: &BatchId) -> Result<(), QueueError>;

    // -- Native batch support --

    /// Return all jobs in the `Processing` state (used by the native-batch poller).
    async fn get_native_jobs_in_progress(&self) -> Result<Vec<BatchJob>, QueueError>;

    // -- Lease management --

    /// Reclaim items whose worker leases have expired so they can be re-claimed.
    /// Returns the number of items reclaimed.
    async fn reclaim_expired_leases(&self) -> Result<u32, QueueError>;

    // -- Progress --

    /// Update the `request_counts` snapshot on a job (called after each item completion).
    async fn update_progress(&self, id: &BatchId, counts: &RequestCounts)
        -> Result<(), QueueError>;

    // -- Items query --

    /// Fetch all items belonging to a batch (used for result retrieval).
    async fn get_items(&self, batch_id: &BatchId) -> Result<Vec<BatchItem>, QueueError>;
}
