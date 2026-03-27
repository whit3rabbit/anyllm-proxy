// Axum handlers for batch file upload and job management.
// POST /v1/files, POST /v1/batches, GET /v1/batches/{id}, GET /v1/batches

use super::db;
use super::validate_jsonl;
use crate::backend::BackendClient;
use crate::server::routes::AppState;
use anyllm_translate::anthropic;
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;

/// POST /v1/files - Upload a JSONL batch file via multipart/form-data.
///
/// Expects fields: `purpose` (must be "batch") and `file` (the JSONL content).
pub async fn upload_file(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => return service_unavailable("Batch storage not available"),
    };

    let mut purpose: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "purpose" => {
                purpose = field.text().await.ok();
            }
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                file_data = field.bytes().await.ok().map(|b| b.to_vec());
            }
            _ => {}
        }
    }

    let purpose = match purpose.as_deref() {
        Some("batch") => "batch",
        Some(other) => {
            return bad_request(&format!(
                "Unsupported purpose: '{other}'. Only 'batch' is supported."
            ));
        }
        None => {
            return bad_request("Missing required field 'purpose'");
        }
    };

    let data = match file_data {
        Some(d) if !d.is_empty() => d,
        _ => {
            return bad_request("Missing or empty 'file' field");
        }
    };

    // Validate JSONL structure
    let validated = match validate_jsonl(&data) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("Invalid JSONL: {e}"));
        }
    };

    let file_id = format!("file-{}", uuid::Uuid::new_v4());
    let byte_size = data.len() as i64;
    let line_count = validated.line_count as i64;

    // Insert into SQLite on the blocking threadpool
    let file_id_clone = file_id.clone();
    let filename_clone = filename.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_batch_tables(&conn)?;
        db::insert_batch_file(
            &conn,
            &file_id_clone,
            None,
            purpose,
            filename_clone.as_deref(),
            byte_size,
            line_count,
            &data,
        )
    })
    .await;

    match result {
        Ok(Ok(())) => {
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            let file_obj = super::BatchFile {
                id: file_id,
                object: "file".to_string(),
                bytes: byte_size,
                created_at: now_epoch,
                filename,
                purpose: purpose.to_string(),
            };
            (StatusCode::OK, Json(file_obj)).into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to store batch file");
            internal_error("Failed to store file")
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking panicked");
            internal_error("Internal error")
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
}

fn default_endpoint() -> String {
    "/v1/chat/completions".to_string()
}

fn default_completion_window() -> String {
    "24h".to_string()
}

/// POST /v1/batches - Create a new batch job.
///
/// Returns 501 for unsupported backends (vertex, gemini, anthropic, bedrock).
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

    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => return service_unavailable("Batch storage not available"),
    };

    let input_file_id = req.input_file_id.clone();
    let batch_id = format!("batch-{}", uuid::Uuid::new_v4());
    let backend_name = state.backend_name.clone();
    let metadata = req.metadata.clone();

    // Verify input file exists and get line count
    let batch_id_clone = batch_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_batch_tables(&conn)?;

        let meta = db::get_batch_file_meta(&conn, &input_file_id)?;
        let (_byte_size, line_count, _created) = match meta {
            Some(m) => m,
            None => {
                return Ok(None);
            }
        };

        db::insert_batch_job(
            &conn,
            &batch_id_clone,
            None,
            &input_file_id,
            &backend_name,
            line_count,
            metadata.as_ref(),
        )?;

        db::get_batch_job(&conn, &batch_id_clone)
    })
    .await;

    match result {
        Ok(Ok(Some(job))) => (StatusCode::OK, Json(job)).into_response(),
        Ok(Ok(None)) => bad_request(&format!("Input file '{}' not found", req.input_file_id)),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to create batch job");
            internal_error("Failed to create batch job")
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking panicked");
            internal_error("Internal error")
        }
    }
}

/// GET /v1/batches/{batch_id} - Retrieve a batch job by ID.
pub async fn get_batch(State(state): State<AppState>, Path(batch_id): Path<String>) -> Response {
    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => return service_unavailable("Batch storage not available"),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_batch_tables(&conn)?;
        db::get_batch_job(&conn, &batch_id)
    })
    .await;

    match result {
        Ok(Ok(Some(job))) => (StatusCode::OK, Json(job)).into_response(),
        Ok(Ok(None)) => {
            let err = create_anthropic_error(
                anthropic::ErrorType::NotFoundError,
                "Batch not found".to_string(),
                None,
            );
            (StatusCode::NOT_FOUND, Json(err)).into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to fetch batch job");
            internal_error("Failed to fetch batch job")
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking panicked");
            internal_error("Internal error")
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

/// GET /v1/batches - List batch jobs with cursor pagination.
pub async fn list_batches(
    State(state): State<AppState>,
    Query(query): Query<ListBatchesQuery>,
) -> Response {
    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => return service_unavailable("Batch storage not available"),
    };

    let limit = query.limit.min(100);
    let after = query.after.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_batch_tables(&conn)?;
        db::list_batch_jobs(&conn, None, limit, after.as_deref())
    })
    .await;

    match result {
        Ok(Ok(jobs)) => {
            let has_more = jobs.len() as u32 == limit;
            let last_id = jobs.last().map(|j| j.id.clone());
            let response = serde_json::json!({
                "object": "list",
                "data": jobs,
                "has_more": has_more,
                "first_id": jobs.first().map(|j| &j.id),
                "last_id": last_id,
            });
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to list batch jobs");
            internal_error("Failed to list batch jobs")
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking panicked");
            internal_error("Internal error")
        }
    }
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
