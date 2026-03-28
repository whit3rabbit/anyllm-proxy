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
///
/// Takes a `BufRead` to read line-by-line without requiring a contiguous UTF-8
/// string for the entire file.
pub fn validate_jsonl(mut reader: impl std::io::BufRead) -> Result<ValidatedJsonl, String> {
    let mut seen_ids = HashSet::new();
    let mut line_count = 0usize;
    let mut raw_line_num = 0usize;
    let mut bytes_read = 0usize;
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let n = reader
            .read_line(&mut line_buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 {
            break; // EOF
        }
        raw_line_num += 1;
        bytes_read += n;
        if bytes_read > MAX_FILE_SIZE {
            return Err(format!(
                "File exceeds maximum size of {} bytes",
                MAX_FILE_SIZE
            ));
        }

        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        line_count += 1;
        if line_count > MAX_LINE_COUNT {
            return Err(format!("File exceeds maximum of {MAX_LINE_COUNT} lines"));
        }

        let parsed: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("Line {raw_line_num}: invalid JSON: {e}"))?;

        let obj = parsed
            .as_object()
            .ok_or_else(|| format!("Line {raw_line_num}: expected JSON object"))?;

        let custom_id = obj
            .get("custom_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Line {raw_line_num}: missing or non-string 'custom_id'"))?;

        if custom_id.len() > MAX_CUSTOM_ID_LEN {
            return Err(format!(
                "Line {raw_line_num}: custom_id exceeds maximum length of {MAX_CUSTOM_ID_LEN} characters"
            ));
        }

        if !seen_ids.insert(custom_id.to_string()) {
            return Err(format!(
                "Line {raw_line_num}: duplicate custom_id '{custom_id}'"
            ));
        }

        let body = obj
            .get("body")
            .and_then(|v| v.as_object())
            .ok_or_else(|| format!("Line {raw_line_num}: missing or non-object 'body'"))?;

        if !body.contains_key("model") {
            return Err(format!("Line {raw_line_num}: body missing 'model' field"));
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
    use std::io::{BufReader, Cursor};

    fn check(data: &str) -> Result<ValidatedJsonl, String> {
        validate_jsonl(BufReader::new(Cursor::new(data.as_bytes())))
    }

    fn check_bytes(data: &[u8]) -> Result<ValidatedJsonl, String> {
        validate_jsonl(BufReader::new(Cursor::new(data)))
    }

    #[test]
    fn valid_jsonl() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o", "messages": []}}
{"custom_id": "req-2", "body": {"model": "gpt-4o", "messages": []}}"#;
        let result = check(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }

    #[test]
    fn missing_custom_id() {
        let data = r#"{"body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("custom_id"));
    }

    #[test]
    fn missing_body_model() {
        let data = r#"{"custom_id": "req-1", "body": {"messages": []}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("model"));
    }

    #[test]
    fn duplicate_custom_id() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}
{"custom_id": "req-1", "body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate"));
    }

    #[test]
    fn oversized_custom_id() {
        let long_id = "a".repeat(65);
        let data = format!(r#"{{"custom_id": "{long_id}", "body": {{"model": "gpt-4o"}}}}"#);
        let result = check(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("maximum length"));
    }

    #[test]
    fn empty_file() {
        let result = check_bytes(b"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn invalid_json_line() {
        let data = b"not json at all";
        let result = check_bytes(data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn blank_lines_skipped() {
        let data = r#"{"custom_id": "req-1", "body": {"model": "gpt-4o"}}

{"custom_id": "req-2", "body": {"model": "gpt-4o"}}"#;
        let result = check(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_count, 2);
    }

    #[test]
    fn error_reports_absolute_line_number_with_blank_lines() {
        // Blank line at position 1, bad JSON at position 2.
        // Should report "Line 2", not "Line 1".
        let data = "\n{\"custom_id\": \"ok\", \"body\": INVALID}";
        let err = check(data).unwrap_err();
        assert!(err.contains("Line 2"), "expected 'Line 2' in: {err}");
    }
}
