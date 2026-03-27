// Batch processing types and JSONL validation.
// Implements OpenAI-compatible batch file upload and job management.

pub mod db;
pub mod routes;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum number of lines in a JSONL batch file.
const MAX_LINE_COUNT: usize = 50_000;

/// Maximum file size in bytes (100 MB).
const MAX_FILE_SIZE: usize = 100 * 1024 * 1024;

/// Maximum length of a custom_id field.
const MAX_CUSTOM_ID_LEN: usize = 64;

/// A batch input file stored in SQLite.
#[derive(Debug, Clone, Serialize)]
pub struct BatchFile {
    pub id: String,
    pub object: String,
    pub bytes: i64,
    pub created_at: i64,
    pub filename: Option<String>,
    pub purpose: String,
}

/// A batch processing job.
#[derive(Debug, Clone, Serialize)]
pub struct BatchJob {
    pub id: String,
    pub object: String,
    pub endpoint: String,
    pub status: BatchStatus,
    pub input_file_id: String,
    pub completion_window: String,
    pub created_at: i64,
    pub request_counts: RequestCounts,
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

/// Counts of requests within a batch job.
#[derive(Debug, Clone, Serialize)]
pub struct RequestCounts {
    pub total: i64,
    pub completed: i64,
    pub failed: i64,
}

/// Batch job lifecycle status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Validating,
    InProgress,
    Completed,
    Failed,
    Expired,
    Cancelling,
    Cancelled,
}

impl BatchStatus {
    /// Convert from the string stored in SQLite.
    pub fn from_str_status(s: &str) -> Self {
        match s {
            "validating" => Self::Validating,
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "expired" => Self::Expired,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    /// Convert to the string stored in SQLite.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validating => "validating",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Result of JSONL validation: line count on success, error message on failure.
#[derive(Debug)]
pub struct ValidatedJsonl {
    pub line_count: usize,
}

/// Validate a JSONL batch file.
///
/// Each line must be valid JSON with a unique `custom_id` (string, max 64 chars)
/// and a `body` object containing a `model` field. Max 50,000 lines, 100 MB.
pub fn validate_jsonl(data: &[u8]) -> Result<ValidatedJsonl, String> {
    if data.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File size {} bytes exceeds maximum of {} bytes",
            data.len(),
            MAX_FILE_SIZE
        ));
    }

    let text = std::str::from_utf8(data).map_err(|e| format!("Invalid UTF-8: {e}"))?;

    let mut seen_ids = HashSet::new();
    let mut line_count = 0usize;

    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        line_count += 1;
        if line_count > MAX_LINE_COUNT {
            return Err(format!("File exceeds maximum of {MAX_LINE_COUNT} lines"));
        }

        let parsed: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("Line {}: invalid JSON: {e}", idx + 1))?;

        let obj = parsed
            .as_object()
            .ok_or_else(|| format!("Line {}: expected JSON object", idx + 1))?;

        // Validate custom_id
        let custom_id = obj
            .get("custom_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Line {}: missing or non-string 'custom_id'", idx + 1))?;

        if custom_id.len() > MAX_CUSTOM_ID_LEN {
            return Err(format!(
                "Line {}: custom_id exceeds maximum length of {MAX_CUSTOM_ID_LEN} characters",
                idx + 1
            ));
        }

        if !seen_ids.insert(custom_id.to_string()) {
            return Err(format!(
                "Line {}: duplicate custom_id '{custom_id}'",
                idx + 1
            ));
        }

        // Validate body.model
        let body = obj
            .get("body")
            .and_then(|v| v.as_object())
            .ok_or_else(|| format!("Line {}: missing or non-object 'body'", idx + 1))?;

        if !body.contains_key("model") {
            return Err(format!("Line {}: body missing 'model' field", idx + 1));
        }
    }

    if line_count == 0 {
        return Err("File is empty".to_string());
    }

    Ok(ValidatedJsonl { line_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_jsonl() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o", "messages": []}}
{"custom_id": "req-2", "body": {"model": "gpt-4o", "messages": []}}"#;
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }

    #[test]
    fn missing_custom_id() {
        let data = r#"{"body": {"model": "gpt-4o"}}"#;
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("custom_id"));
    }

    #[test]
    fn missing_body_model() {
        let data = r#"{"custom_id": "req-1", "body": {"messages": []}}"#;
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("model"));
    }

    #[test]
    fn duplicate_custom_id() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}
{"custom_id": "req-1", "body": {"model": "gpt-4o"}}"#;
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate"));
    }

    #[test]
    fn oversized_custom_id() {
        let long_id = "a".repeat(65);
        let data = format!(r#"{{"custom_id": "{long_id}", "body": {{"model": "gpt-4o"}}}}"#);
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("maximum length"));
    }

    #[test]
    fn empty_file() {
        let result = validate_jsonl(b"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn invalid_json_line() {
        let data = b"not json at all";
        let result = validate_jsonl(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn blank_lines_skipped() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}

{"custom_id": "req-2", "body": {"model": "gpt-4o"}}"#;
        let result = validate_jsonl(data.as_bytes());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }
}
