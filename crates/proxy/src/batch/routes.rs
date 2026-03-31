// Axum handlers for batch file upload and job management.
// POST /v1/files, POST /v1/batches, GET /v1/batches/{id}, GET /v1/batches

use crate::backend::BackendClient;
use crate::server::routes::AppState;
use anyllm_batch_engine::job::{BatchSubmission, ExecutionMode, SourceFormat, SubmissionItem};
use anyllm_translate::anthropic;
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use std::io::{BufReader, Cursor};

/// POST /v1/files - Upload a JSONL batch file via multipart/form-data.
pub async fn upload_file(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let engine = match state.batch_engine.as_ref() {
        Some(e) => e.clone(),
        None => return service_unavailable("Batch storage not available"),
    };

    let mut purpose: Option<String> = None;
    let mut file_data: Option<bytes::Bytes> = None;
    let mut filename: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "purpose" => purpose = field.text().await.ok(),
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                file_data = field.bytes().await.ok();
            }
            _ => {}
        }
    }

    match purpose.as_deref() {
        Some("batch") => {}
        Some(other) => {
            return bad_request(&format!(
                "Unsupported purpose: '{other}'. Only 'batch' is supported."
            ));
        }
        None => return bad_request("Missing required field 'purpose'"),
    }

    let data = match file_data {
        Some(d) if !d.is_empty() => d,
        _ => return bad_request("Missing or empty 'file' field"),
    };

    let validated =
        match anyllm_batch_engine::validate_jsonl(BufReader::new(Cursor::new(data.as_ref()))) {
            Ok(v) => v,
            Err(e) => return bad_request(&format!("Invalid JSONL: {e}")),
        };

    let file_id = format!("file-{}", uuid::Uuid::new_v4());
    let byte_size = data.len() as i64;
    let line_count = validated.line_count as i64;

    match engine
        .file_store
        .insert(
            &file_id,
            None,
            filename.as_deref(),
            data.as_ref(),
            line_count,
        )
        .await
    {
        Ok(()) => {
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let file_obj = serde_json::json!({
                "id": file_id,
                "object": "file",
                "bytes": byte_size,
                "created_at": now_epoch,
                "filename": filename,
                "purpose": "batch",
            });
            (StatusCode::OK, Json(file_obj)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to store batch file");
            internal_error("Failed to store file")
        }
    }
}

/// Request body for POST /v1/batches.
#[derive(Deserialize)]
pub struct CreateBatchRequest {
    pub input_file_id: String,
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_completion_window")]
    pub completion_window: String,
    pub metadata: Option<serde_json::Value>,
    /// Optional per-batch webhook URL. Must be a public HTTP(S) URL.
    /// Validated against SSRF rules (rejects private/loopback/metadata IPs).
    pub webhook_url: Option<String>,
}

fn default_endpoint() -> String {
    "/v1/chat/completions".to_string()
}

fn default_completion_window() -> String {
    "24h".to_string()
}

/// POST /v1/batches - Create a new batch job.
pub async fn create_batch(
    State(state): State<AppState>,
    Json(req): Json<CreateBatchRequest>,
) -> Response {
    // Check backend support: only openai and azure are supported
    if !is_batch_supported(&state.backend) {
        return not_implemented(&format!(
            "Batch processing is not supported by the '{}' backend",
            state.backend_name
        ));
    }

    let engine = match state.batch_engine.as_ref() {
        Some(e) => e.clone(),
        None => return service_unavailable("Batch storage not available"),
    };

    // Read file content from file store.
    let content = match engine.file_store.get_content(&req.input_file_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return bad_request(&format!("Input file '{}' not found", req.input_file_id)),
        Err(e) => {
            tracing::error!(error = %e, "failed to read batch file content");
            return internal_error("Failed to read file");
        }
    };

    // Parse JSONL into submission items.
    let items: Vec<SubmissionItem> = match parse_jsonl_items(&content) {
        Ok(items) => items,
        Err(e) => return bad_request(&format!("Invalid JSONL: {e}")),
    };

    // Reject private/loopback/metadata targets to prevent SSRF.
    if let Some(ref url) = req.webhook_url {
        if let Err(e) = crate::config::validate_base_url(url) {
            return bad_request(&format!("Invalid webhook_url: {e}"));
        }
    }

    let execution_mode = if is_batch_supported(&state.backend) {
        ExecutionMode::Native {
            provider: state.backend_name.clone(),
        }
    } else {
        ExecutionMode::ProxyNative
    };

    let submission = BatchSubmission {
        items,
        execution_mode,
        input_file_id: req.input_file_id.clone(),
        key_id: None,
        webhook_url: req.webhook_url.clone(),
        metadata: req.metadata.clone(),
        priority: 0,
    };

    match engine.submit(submission).await {
        Ok(job) => (StatusCode::OK, Json(job_to_openai_response(&job))).into_response(),
        Err(anyllm_batch_engine::EngineError::FileNotFound(_)) => {
            bad_request(&format!("Input file '{}' not found", req.input_file_id))
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to create batch job");
            internal_error("Failed to create batch job")
        }
    }
}

/// GET /v1/batches/{batch_id}
pub async fn get_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    let engine = match state.batch_engine.as_ref() {
        Some(e) => e.clone(),
        None => return service_unavailable("Batch storage not available"),
    };

    match engine.get(&anyllm_batch_engine::BatchId(batch_id)).await {
        Ok(Some(job)) => (StatusCode::OK, Json(job_to_openai_response(&job))).into_response(),
        Ok(None) => {
            let err = create_anthropic_error(
                anthropic::ErrorType::NotFoundError,
                "Batch not found".to_string(),
                None,
            );
            (StatusCode::NOT_FOUND, Json(err)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to fetch batch job");
            internal_error("Failed to fetch batch job")
        }
    }
}

/// Query parameters for GET /v1/batches.
#[derive(Deserialize)]
pub struct ListBatchesQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    pub after: Option<String>,
}

fn default_limit() -> u32 {
    20
}

/// GET /v1/batches
pub async fn list_batches(
    State(state): State<AppState>,
    Query(query): Query<ListBatchesQuery>,
) -> Response {
    let engine = match state.batch_engine.as_ref() {
        Some(e) => e.clone(),
        None => return service_unavailable("Batch storage not available"),
    };

    let limit = query.limit.min(100);

    match engine.list(None, query.after.as_deref(), limit).await {
        Ok(jobs) => {
            let has_more = jobs.len() as u32 == limit;
            let first_id = jobs.first().map(|j| j.id.0.clone());
            let last_id = jobs.last().map(|j| j.id.0.clone());
            let data: Vec<serde_json::Value> = jobs.iter().map(job_to_openai_response).collect();
            let response = serde_json::json!({
                "object": "list",
                "data": data,
                "has_more": has_more,
                "first_id": first_id,
                "last_id": last_id,
            });
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to list batch jobs");
            internal_error("Failed to list batch jobs")
        }
    }
}

/// POST /v1/batches/{batch_id}/cancel
pub async fn cancel_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    let Some(engine) = state.batch_engine.as_ref() else {
        return not_implemented("batch engine not available");
    };

    let id = anyllm_batch_engine::BatchId(batch_id);
    match engine.cancel(&id).await {
        Ok(job) => (StatusCode::OK, Json(job_to_openai_response(&job))).into_response(),
        Err(anyllm_batch_engine::EngineError::Queue(anyllm_batch_engine::QueueError::NotFound)) => {
            not_found_response("batch not found")
        }
        Err(e) => internal_error(&e.to_string()),
    }
}

/// Map a BatchJob to an OpenAI-compatible batch response JSON.
pub fn job_to_openai_response(job: &anyllm_batch_engine::BatchJob) -> serde_json::Value {
    let created_epoch = iso8601_to_epoch(&job.created_at);
    let completed_epoch = job.completed_at.as_deref().map(iso8601_to_epoch);

    serde_json::json!({
        "id": job.id.0,
        "object": "batch",
        "endpoint": "/v1/chat/completions",
        "status": map_batch_status(&job.status),
        "input_file_id": job.input_file_id,
        "completion_window": "24h",
        "created_at": created_epoch,
        "request_counts": {
            "total": job.request_counts.total,
            "completed": job.request_counts.succeeded,
            "failed": job.request_counts.failed,
        },
        "metadata": job.metadata,
        "output_file_id": serde_json::Value::Null,
        "error_file_id": serde_json::Value::Null,
        "completed_at": completed_epoch,
    })
}

/// Map BatchEngine status to OpenAI batch status string.
fn map_batch_status(status: &anyllm_batch_engine::BatchStatus) -> &'static str {
    match status {
        anyllm_batch_engine::BatchStatus::Queued => "validating",
        anyllm_batch_engine::BatchStatus::Processing => "in_progress",
        anyllm_batch_engine::BatchStatus::Completed => "completed",
        anyllm_batch_engine::BatchStatus::Failed => "failed",
        anyllm_batch_engine::BatchStatus::Cancelling => "cancelling",
        anyllm_batch_engine::BatchStatus::Cancelled => "cancelled",
        anyllm_batch_engine::BatchStatus::Expired => "expired",
    }
}

/// Parse JSONL bytes into SubmissionItems.
fn parse_jsonl_items(content: &[u8]) -> Result<Vec<SubmissionItem>, String> {
    let mut items = Vec::new();
    let text = std::str::from_utf8(content).map_err(|e| format!("Invalid UTF-8: {e}"))?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("Invalid JSON: {e}"))?;
        let obj = parsed.as_object().ok_or("Expected JSON object")?;
        let custom_id = obj
            .get("custom_id")
            .and_then(|v| v.as_str())
            .ok_or("Missing custom_id")?
            .to_string();
        let body = obj.get("body").cloned().unwrap_or(serde_json::Value::Null);
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        items.push(SubmissionItem {
            custom_id,
            model,
            body,
            source_format: SourceFormat::OpenAI,
        });
    }
    Ok(items)
}

fn iso8601_to_epoch(s: &str) -> i64 {
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return 0;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_str = parts[1].trim_end_matches('Z');
    let time_parts: Vec<u64> = time_str.split(':').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return 0;
    }
    let (y, m, d) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hh, mm, ss) = (time_parts[0], time_parts[1], time_parts[2]);
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj / 400;
    let yoe = y_adj - era * 400;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + hh * 3600 + mm * 60 + ss) as i64
}

/// Check if the backend supports batch processing (OpenAI and Azure only).
fn is_batch_supported(backend: &BackendClient) -> bool {
    matches!(
        backend,
        BackendClient::OpenAI(_) | BackendClient::AzureOpenAI(_)
    )
}

fn bad_request(msg: &str) -> Response {
    let err = create_anthropic_error(
        anthropic::ErrorType::InvalidRequestError,
        msg.to_string(),
        None,
    );
    (StatusCode::BAD_REQUEST, Json(err)).into_response()
}

fn not_implemented(msg: &str) -> Response {
    let err = create_anthropic_error(
        anthropic::ErrorType::InvalidRequestError,
        msg.to_string(),
        None,
    );
    (StatusCode::NOT_IMPLEMENTED, Json(err)).into_response()
}

fn service_unavailable(msg: &str) -> Response {
    let err = create_anthropic_error(anthropic::ErrorType::ApiError, msg.to_string(), None);
    (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response()
}

fn internal_error(msg: &str) -> Response {
    let err = create_anthropic_error(anthropic::ErrorType::ApiError, msg.to_string(), None);
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
}

fn not_found_response(msg: &str) -> Response {
    let err = create_anthropic_error(anthropic::ErrorType::NotFoundError, msg.to_string(), None);
    (StatusCode::NOT_FOUND, Json(err)).into_response()
}

#[cfg(test)]
mod tests {
    #[test]
    fn validate_webhook_url_rejects_private_ip() {
        let result = crate::config::validate_base_url(
            "http://169.254.169.254/metadata",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private/loopback"));
    }

    #[test]
    fn validate_webhook_url_accepts_public_https() {
        let result =
            crate::config::validate_base_url("https://hooks.example.com/notify");
        assert!(result.is_ok());
    }
}
