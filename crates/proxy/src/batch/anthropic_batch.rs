// crates/proxy/src/batch/anthropic_batch.rs
// Route handlers for Anthropic native batch API.
//
// POST /v1/messages/batches           — translate and submit to OpenAI batch API
// GET  /v1/messages/batches/{id}      — poll status
// GET  /v1/messages/batches/{id}/results — download and translate output JSONL

use super::db;
use super::openai_batch_client::{openai_batch_to_message_batch, OpenAIBatchClient};
use crate::backend::BackendClient;
use crate::server::routes::{AnthropicJson, AppState};
use anyllm_translate::anthropic::batch::CreateBatchRequest;
use anyllm_translate::anthropic::errors::ErrorType;
use anyllm_translate::mapping::batch_map::{
    translate_batch_to_openai_jsonl, translate_openai_result_line,
};
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

/// Extract the OpenAI API key and base URL from the backend client.
/// Returns None when the backend does not support batch (Vertex, Gemini, Anthropic, Bedrock).
fn extract_openai_credentials(backend: &BackendClient) -> Option<(String, String)> {
    match backend {
        BackendClient::OpenAI(c)
        | BackendClient::AzureOpenAI(c)
        | BackendClient::OpenAIResponses(c) => Some((c.api_key(), c.base_url_for_batch())),
        _ => None,
    }
}

/// POST /v1/messages/batches
pub(crate) async fn create_anthropic_batch(
    State(state): State<AppState>,
    AnthropicJson(req): AnthropicJson<CreateBatchRequest>,
) -> Response {
    if req.requests.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ErrorType::InvalidRequestError,
            "requests array must not be empty",
        );
    }

    let (api_key, base_url) = match extract_openai_credentials(&state.backend) {
        Some(creds) => creds,
        None => {
            return error_response(
                StatusCode::NOT_IMPLEMENTED,
                ErrorType::InvalidRequestError,
                "Batch processing is only supported for OpenAI and Azure backends",
            );
        }
    };

    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorType::ApiError,
                "Batch storage not available",
            );
        }
    };

    // Derive model name from first request (all should use the same model after mapping).
    let model = req.requests[0].params.model.clone();

    // Translate Anthropic JSONL to OpenAI JSONL.
    let openai_jsonl = translate_batch_to_openai_jsonl(&req.requests);

    let batch_client = OpenAIBatchClient::new(api_key, base_url);

    // Upload translated JSONL to OpenAI.
    let openai_file_id = match batch_client.upload_jsonl_file(&openai_jsonl).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "failed to upload batch file to OpenAI");
            return error_response(StatusCode::BAD_GATEWAY, ErrorType::ApiError, &e);
        }
    };

    // Create OpenAI batch job.
    let openai_batch_id = match batch_client.create_batch(&openai_file_id).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "failed to create OpenAI batch job");
            return error_response(StatusCode::BAD_GATEWAY, ErrorType::ApiError, &e);
        }
    };

    // Generate our Anthropic-format batch ID.
    let our_batch_id = format!("msgbatch_{}", uuid::Uuid::new_v4().as_simple());

    // Store the mapping in SQLite.
    let our_id = our_batch_id.clone();
    let oai_id = openai_batch_id.clone();
    let model_clone = model.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_anthropic_batch_map_table(&conn)?;
        db::insert_anthropic_batch_map(&conn, &our_id, &oai_id)?;
        // Store model alongside mapping for result translation.
        conn.execute(
            "UPDATE anthropic_batch_map SET model = ?1 WHERE our_batch_id = ?2",
            rusqlite::params![model_clone, our_id],
        )?;
        Ok::<_, rusqlite::Error>(())
    })
    .await;

    if let Err(e) = result {
        tracing::error!(error = %e, "spawn_blocking panicked storing batch mapping");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorType::ApiError,
            "storage error",
        );
    }
    if let Ok(Err(e)) = result {
        tracing::error!(error = %e, "failed to store batch mapping");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorType::ApiError,
            "storage error",
        );
    }

    // Poll OpenAI to get the initial batch status and translate it.
    match batch_client.get_batch_status(&openai_batch_id).await {
        Ok(v) => {
            let batch = openai_batch_to_message_batch(&our_batch_id, &v);
            (StatusCode::OK, Json(batch)).into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not poll initial batch status");
            // Return a synthetic in-progress response — the batch was submitted successfully.
            use anyllm_translate::anthropic::batch::{
                BatchRequestCounts, MessageBatch, ProcessingStatus,
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let batch = MessageBatch {
                id: our_batch_id,
                type_: "message_batch".to_string(),
                processing_status: ProcessingStatus::InProgress,
                request_counts: BatchRequestCounts {
                    processing: req.requests.len() as u32,
                    ..Default::default()
                },
                ended_at: None,
                created_at: now,
                expires_at: now + 86400,
                archived_at: None,
                cancel_initiated_at: None,
                results_url: None,
            };
            (StatusCode::OK, Json(batch)).into_response()
        }
    }
}

/// GET /v1/messages/batches/{id}
pub(crate) async fn get_anthropic_batch(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Response {
    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorType::ApiError,
                "storage unavailable",
            );
        }
    };

    let batch_id_clone = batch_id.clone();
    let mapping = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::init_anthropic_batch_map_table(&conn)?;
        db::get_anthropic_batch_map(&conn, &batch_id_clone)
    })
    .await;

    let map = match mapping {
        Ok(Ok(Some(m))) => m,
        Ok(Ok(None)) => {
            return error_response(
                StatusCode::NOT_FOUND,
                ErrorType::NotFoundError,
                "batch not found",
            );
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "db error fetching batch map");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorType::ApiError,
                "storage error",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking panicked");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorType::ApiError,
                "internal error",
            );
        }
    };

    let (api_key, base_url) = match extract_openai_credentials(&state.backend) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::NOT_IMPLEMENTED,
                ErrorType::InvalidRequestError,
                "backend not supported",
            );
        }
    };

    let batch_client = OpenAIBatchClient::new(api_key, base_url);
    match batch_client.get_batch_status(&map.openai_batch_id).await {
        Ok(v) => {
            let batch = openai_batch_to_message_batch(&batch_id, &v);
            (StatusCode::OK, Json(batch)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to poll OpenAI batch status");
            error_response(StatusCode::BAD_GATEWAY, ErrorType::ApiError, &e)
        }
    }
}

/// GET /v1/messages/batches/{id}/results
///
/// Downloads the OpenAI output JSONL and streams Anthropic-format result lines.
pub(crate) async fn get_anthropic_batch_results(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Response {
    let db = match state.shared.as_ref().map(|s| s.db.clone()) {
        Some(db) => db,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorType::ApiError,
                "storage unavailable",
            );
        }
    };

    let batch_id_clone = batch_id.clone();
    let mapping = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        db::get_anthropic_batch_map(&conn, &batch_id_clone)
    })
    .await;

    let map = match mapping {
        Ok(Ok(Some(m))) => m,
        Ok(Ok(None)) => {
            return error_response(
                StatusCode::NOT_FOUND,
                ErrorType::NotFoundError,
                "batch not found",
            );
        }
        _ => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorType::ApiError,
                "storage error",
            );
        }
    };

    let (api_key, base_url) = match extract_openai_credentials(&state.backend) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::NOT_IMPLEMENTED,
                ErrorType::InvalidRequestError,
                "backend not supported",
            );
        }
    };

    let batch_client = OpenAIBatchClient::new(api_key, base_url);

    // Determine output file id: use cached one or poll OpenAI.
    let output_file_id = if let Some(fid) = map.openai_output_file_id {
        fid
    } else {
        let status = match batch_client.get_batch_status(&map.openai_batch_id).await {
            Ok(v) => v,
            Err(e) => {
                return error_response(StatusCode::BAD_GATEWAY, ErrorType::ApiError, &e);
            }
        };
        match status["output_file_id"].as_str() {
            Some(fid) => fid.to_string(),
            None => {
                return error_response(
                    StatusCode::CONFLICT,
                    ErrorType::InvalidRequestError,
                    "batch results are not yet available; poll GET /v1/messages/batches/{id} for status",
                );
            }
        }
    };

    // Download the output JSONL from OpenAI.
    let openai_jsonl = match batch_client.get_file_content(&output_file_id).await {
        Ok(content) => content,
        Err(e) => {
            return error_response(StatusCode::BAD_GATEWAY, ErrorType::ApiError, &e);
        }
    };

    // Translate each line to Anthropic format.
    let model = if map.model.is_empty() {
        "claude-3-5-sonnet-20241022"
    } else {
        &map.model
    };
    let anthropic_lines: Vec<String> = openai_jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            translate_openai_result_line(line, model).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "skipping untranslatable result line");
                // Emit an errored result for lines that fail translation.
                serde_json::json!({
                    "custom_id": "unknown",
                    "result": {
                        "type": "errored",
                        "error": {"type": "api_error", "message": e}
                    }
                })
                .to_string()
            })
        })
        .collect();

    let body = anthropic_lines.join("\n");
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-jsonl")
        .body(axum::body::Body::from(body))
        .unwrap()
        .into_response()
}

fn error_response(status: StatusCode, error_type: ErrorType, msg: &str) -> Response {
    let err = create_anthropic_error(error_type, msg.to_string(), None);
    (status, Json(err)).into_response()
}
