use crate::admin::db::{delete_favorite, insert_favorite, list_favorites};
use crate::admin::state::{with_db, SharedState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

#[derive(serde::Deserialize)]
pub struct FavoriteRequest {
    pub provider_id: String,
}

fn db_error() -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "favorites database error" })),
    )
        .into_response()
}

/// GET /admin/api/favorites -> { "favorites": ["ollama", ...] }
pub async fn list(State(shared): State<SharedState>) -> axum::response::Response {
    match with_db(&shared.db, list_favorites).await {
        Some(Ok(ids)) => Json(serde_json::json!({ "favorites": ids })).into_response(),
        _ => db_error().into_response(),
    }
}

/// POST /admin/api/favorites { provider_id } -> 204. Unknown provider ids are rejected.
pub async fn create(
    State(shared): State<SharedState>,
    Json(body): Json<FavoriteRequest>,
) -> axum::response::Response {
    if shared
        .provider_catalog
        .get_provider(&body.provider_id)
        .is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "unknown provider_id" })),
        )
            .into_response();
    }
    let id = body.provider_id;
    match with_db(&shared.db, move |c| insert_favorite(c, &id)).await {
        Some(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => db_error().into_response(),
    }
}

/// DELETE /admin/api/favorites/{provider_id} -> 204.
pub async fn delete(
    State(shared): State<SharedState>,
    Path(provider_id): Path<String>,
) -> axum::response::Response {
    match with_db(&shared.db, move |c| delete_favorite(c, &provider_id)).await {
        Some(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        _ => db_error().into_response(),
    }
}
