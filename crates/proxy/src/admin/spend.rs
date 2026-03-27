// Admin endpoint for per-key spend reporting.
//
// GET /admin/api/keys/{id}/spend returns accumulated cost and token usage
// for a single virtual API key.

use crate::admin::state::SharedState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

/// GET /admin/api/keys/{id}/spend -- per-key cost and usage summary.
pub async fn get_key_spend(
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::cost::db::get_key_spend(conn, id)
    })
    .await;

    match result {
        Some(Ok(Some(spend))) => (StatusCode::OK, Json(serde_json::json!(spend))).into_response(),
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found"})),
        )
            .into_response(),
        Some(Err(e)) => {
            tracing::error!(error = %e, "get_key_spend failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal database error"})),
            )
                .into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response(),
    }
}
