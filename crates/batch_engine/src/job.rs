// crates/batch_engine/src/job.rs
//! Core batch orchestration types. HTTP-agnostic.

use serde::{Deserialize, Serialize};

/// Unique batch job identifier. Format: "batch_{uuid}".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BatchId(pub String);

/// Unique item identifier within a batch. Format: "item_{uuid}".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub String);

impl BatchId {
    pub fn new() -> Self {
        Self(format!("batch_{}", uuid::Uuid::new_v4()))
    }
}

impl Default for BatchId {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemId {
    pub fn new() -> Self {
        Self(format!("item_{}", uuid::Uuid::new_v4()))
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Batch job lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    Expired,
}

impl BatchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn from_str_status(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "processing" => Self::Processing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            "expired" => Self::Expired,
            _ => Self::Failed,
        }
    }

    /// Whether this status is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Expired
        )
    }
}

/// How the batch will be executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExecutionMode {
    /// Delegate to provider's native batch API (OpenAI, Azure).
    Native { provider: String },
    /// Proxy processes items individually against the backend.
    ProxyNative,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Native { .. } => "native",
            Self::ProxyNative => "proxy_native",
        }
    }

    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Native { provider } => Some(provider),
            Self::ProxyNative => None,
        }
    }
}

/// A batch job as seen by the engine.
#[derive(Debug, Clone, Serialize)]
pub struct BatchJob {
    pub id: BatchId,
    pub status: BatchStatus,
    pub execution_mode: ExecutionMode,
    pub priority: u8,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub request_counts: RequestCounts,
    pub input_file_id: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub expires_at: String,
}

/// Counts of requests within a batch job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestCounts {
    pub total: u32,
    pub processing: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub expired: u32,
}

/// Single item within a batch.
#[derive(Debug, Clone)]
pub struct BatchItem {
    pub id: ItemId,
    pub batch_id: BatchId,
    pub custom_id: String,
    pub status: ItemStatus,
    pub request: BatchItemRequest,
    pub result: Option<BatchItemResult>,
    pub attempts: u8,
    pub max_retries: u8,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub lease_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Processing,
    Succeeded,
    Failed,
    Cancelled,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str_status(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "processing" => Self::Processing,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// The LLM request payload for a batch item.
#[derive(Debug, Clone)]
pub struct BatchItemRequest {
    pub model: String,
    pub body: serde_json::Value,
    pub source_format: SourceFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Anthropic,
    OpenAI,
}

/// Result of executing a single batch item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemResult {
    pub status_code: u16,
    pub body: serde_json::Value,
}

/// Submission request to the engine (from proxy handlers).
pub struct BatchSubmission {
    pub items: Vec<SubmissionItem>,
    pub execution_mode: ExecutionMode,
    pub input_file_id: String,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub priority: u8,
}

/// A single item in a batch submission.
pub struct SubmissionItem {
    pub custom_id: String,
    pub model: String,
    pub body: serde_json::Value,
    pub source_format: SourceFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_id_format() {
        let id = BatchId::new();
        assert!(id.0.starts_with("batch_"));
        assert_eq!(id.0.len(), 6 + 36); // "batch_" + uuid
    }

    #[test]
    fn item_id_format() {
        let id = ItemId::new();
        assert!(id.0.starts_with("item_"));
    }

    #[test]
    fn batch_status_roundtrip() {
        for status in [
            BatchStatus::Queued,
            BatchStatus::Processing,
            BatchStatus::Completed,
            BatchStatus::Failed,
            BatchStatus::Cancelling,
            BatchStatus::Cancelled,
            BatchStatus::Expired,
        ] {
            let s = status.as_str();
            let parsed = BatchStatus::from_str_status(s);
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn terminal_statuses() {
        assert!(!BatchStatus::Queued.is_terminal());
        assert!(!BatchStatus::Processing.is_terminal());
        assert!(BatchStatus::Completed.is_terminal());
        assert!(BatchStatus::Failed.is_terminal());
        assert!(BatchStatus::Cancelled.is_terminal());
        assert!(BatchStatus::Expired.is_terminal());
    }

    #[test]
    fn execution_mode_str() {
        let native = ExecutionMode::Native {
            provider: "openai".into(),
        };
        assert_eq!(native.as_str(), "native");
        assert_eq!(native.provider(), Some("openai"));

        let proxy = ExecutionMode::ProxyNative;
        assert_eq!(proxy.as_str(), "proxy_native");
        assert_eq!(proxy.provider(), None);
    }
}
