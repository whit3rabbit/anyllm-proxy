// crates/proxy/src/batch/openai_batch_client.rs
// HTTP client for OpenAI batch API endpoints (/v1/files, /v1/batches).
// Stateless: each method takes the credentials stored in OpenAIBatchClient.

use anyllm_translate::anthropic::batch::{BatchRequestCounts, MessageBatch, ProcessingStatus};
use reqwest::{multipart, Client, StatusCode};
use std::time::{SystemTime, UNIX_EPOCH};

/// Thin HTTP client for OpenAI file and batch APIs.
///
/// Constructed once per request from `AppState` config values.
pub struct OpenAIBatchClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAIBatchClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url,
        }
    }

    pub fn files_url(&self) -> String {
        format!("{}/v1/files", self.base_url.trim_end_matches('/'))
    }

    pub fn batches_url(&self) -> String {
        format!("{}/v1/batches", self.base_url.trim_end_matches('/'))
    }

    fn batch_url(&self, batch_id: &str) -> String {
        format!("{}/{}", self.batches_url(), batch_id)
    }

    fn file_content_url(&self, file_id: &str) -> String {
        format!("{}/{}/content", self.files_url(), file_id)
    }

    /// Upload a JSONL string as a file with purpose=batch.
    /// Returns the OpenAI file_id (e.g. "file-abc123").
    pub async fn upload_jsonl_file(&self, jsonl: &str) -> Result<String, String> {
        let part = multipart::Part::text(jsonl.to_string())
            .file_name("batch.jsonl")
            .mime_str("application/jsonl")
            .map_err(|e| format!("mime error: {e}"))?;

        let form = multipart::Form::new()
            .text("purpose", "batch")
            .part("file", part);

        let resp = self
            .client
            .post(&self.files_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("upload request failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("file upload failed: {body}"));
        }

        let v: serde_json::Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;
        v["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "missing id in file upload response".to_string())
    }

    /// Create an OpenAI batch job from an uploaded file.
    /// Returns the OpenAI batch_id (e.g. "batch_abc123").
    pub async fn create_batch(&self, input_file_id: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "input_file_id": input_file_id,
            "endpoint": "/v1/chat/completions",
            "completion_window": "24h"
        });

        let resp = self
            .client
            .post(&self.batches_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("batch create failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("batch creation failed: {body}"));
        }

        let v: serde_json::Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;
        v["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "missing id in batch response".to_string())
    }

    /// Poll status of an OpenAI batch job. Returns the raw JSON value.
    pub async fn get_batch_status(
        &self,
        openai_batch_id: &str,
    ) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .get(&self.batch_url(openai_batch_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("get batch failed: {e}"))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Err("batch not found at OpenAI".to_string());
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("get batch failed: {body}"));
        }

        resp.json().await.map_err(|e| format!("parse error: {e}"))
    }

    /// Download the content of an OpenAI file by file_id. Returns raw bytes as String.
    pub async fn get_file_content(&self, file_id: &str) -> Result<String, String> {
        let resp = self
            .client
            .get(&self.file_content_url(file_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("file download failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("file download failed: {body}"));
        }

        resp.text()
            .await
            .map_err(|e| format!("read body failed: {e}"))
    }
}

/// Map an OpenAI batch status string to Anthropic ProcessingStatus.
pub fn openai_status_to_processing_status(status: &str) -> ProcessingStatus {
    match status {
        "in_progress" | "validating" | "finalizing" => ProcessingStatus::InProgress,
        "cancelling" => ProcessingStatus::Canceling,
        _ => ProcessingStatus::Ended, // completed, failed, expired, cancelled
    }
}

/// Map an OpenAI batch JSON response to an Anthropic MessageBatch.
pub fn openai_batch_to_message_batch(our_batch_id: &str, v: &serde_json::Value) -> MessageBatch {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let status_str = v["status"].as_str().unwrap_or("in_progress");
    let processing_status = openai_status_to_processing_status(status_str);

    let request_counts = BatchRequestCounts {
        processing: v["request_counts"]["in_progress"].as_u64().unwrap_or(0) as u32
            + v["request_counts"]["validating"].as_u64().unwrap_or(0) as u32,
        succeeded: v["request_counts"]["completed"].as_u64().unwrap_or(0) as u32,
        errored: v["request_counts"]["failed"].as_u64().unwrap_or(0) as u32,
        canceled: v["request_counts"]["cancelled"].as_u64().unwrap_or(0) as u32,
        expired: v["request_counts"]["expired"].as_u64().unwrap_or(0) as u32,
    };

    let ended_at = if matches!(processing_status, ProcessingStatus::Ended) {
        v["completed_at"].as_i64().or(Some(now))
    } else {
        None
    };

    MessageBatch {
        id: our_batch_id.to_string(),
        type_: "message_batch".to_string(),
        processing_status,
        request_counts,
        ended_at,
        created_at: v["created_at"].as_i64().unwrap_or(now),
        expires_at: v["expires_at"].as_i64().unwrap_or(now + 86400),
        archived_at: None,
        cancel_initiated_at: None,
        results_url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_file_upload_url() {
        let c = OpenAIBatchClient::new(
            "sk-test".to_string(),
            "https://api.openai.com".to_string(),
        );
        assert_eq!(c.files_url(), "https://api.openai.com/v1/files");
        assert_eq!(c.batches_url(), "https://api.openai.com/v1/batches");
    }

    #[test]
    fn parse_openai_batch_status_to_anthropic() {
        let openai_status = "completed";
        assert!(matches!(
            openai_status_to_processing_status(openai_status),
            ProcessingStatus::Ended
        ));
        let openai_status = "in_progress";
        assert!(matches!(
            openai_status_to_processing_status(openai_status),
            ProcessingStatus::InProgress
        ));
    }
}
