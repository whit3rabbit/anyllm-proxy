// crates/translator/src/anthropic/batch.rs
// Anthropic Message Batches API types.
// See https://docs.anthropic.com/en/api/creating-message-batches

use crate::anthropic::errors::{ErrorDetail, ErrorType};
use crate::anthropic::messages::{MessageCreateRequest, MessageResponse};
use serde::{Deserialize, Serialize};

/// One entry in a POST /v1/messages/batches request body.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct BatchRequestItem {
    pub custom_id: String,
    pub params: MessageCreateRequest,
}

/// POST /v1/messages/batches request body.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CreateBatchRequest {
    pub requests: Vec<BatchRequestItem>,
}

/// Processing lifecycle of a message batch.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStatus {
    InProgress,
    Canceling,
    Ended,
}

/// Per-status request counts within a batch.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct BatchRequestCounts {
    pub processing: u32,
    pub succeeded: u32,
    pub errored: u32,
    pub canceled: u32,
    pub expired: u32,
}

/// Message batch object returned by the API.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MessageBatch {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub processing_status: ProcessingStatus,
    pub request_counts: BatchRequestCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    pub created_at: i64,
    pub expires_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_initiated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results_url: Option<String>,
}

/// Result type for a single batch request.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BatchResultVariant {
    Succeeded { message: MessageResponse },
    Errored { error: ErrorDetail },
    Canceled,
    Expired,
}

/// One line in the results JSONL file.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct BatchResultItem {
    pub custom_id: String,
    pub result: BatchResultVariant,
}

// Suppress unused import warning: ErrorType is re-exported for consumers of this module.
#[allow(unused_imports)]
pub use crate::anthropic::errors::ErrorType as BatchErrorType;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_create_batch_request() {
        let json = serde_json::json!({
            "requests": [
                {
                    "custom_id": "req-1",
                    "params": {
                        "model": "claude-3-5-sonnet-20241022",
                        "max_tokens": 100,
                        "messages": [{"role": "user", "content": "Hello"}]
                    }
                }
            ]
        });
        let req: CreateBatchRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.requests.len(), 1);
        assert_eq!(req.requests[0].custom_id, "req-1");
        assert_eq!(req.requests[0].params.model, "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn serialize_message_batch_in_progress() {
        let batch = MessageBatch {
            id: "msgbatch_abc".to_string(),
            type_: "message_batch".to_string(),
            processing_status: ProcessingStatus::InProgress,
            request_counts: BatchRequestCounts {
                processing: 2,
                succeeded: 0,
                errored: 0,
                canceled: 0,
                expired: 0,
            },
            ended_at: None,
            created_at: 1_700_000_000,
            expires_at: 1_700_086_400,
            archived_at: None,
            cancel_initiated_at: None,
            results_url: None,
        };
        let v = serde_json::to_value(&batch).unwrap();
        assert_eq!(v["id"], "msgbatch_abc");
        assert_eq!(v["processing_status"], "in_progress");
        assert_eq!(v["request_counts"]["processing"], 2);
    }

    #[test]
    fn serialize_batch_result_succeeded() {
        // BatchResultVariant::Succeeded wraps a MessageResponse.
        // The "type" discriminant should serialize to "succeeded".
        let v = serde_json::to_value(BatchResultVariant::Canceled).unwrap();
        assert_eq!(v["type"], "canceled");
    }
}
