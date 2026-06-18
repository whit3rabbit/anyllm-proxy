use super::config::row_to_backend_config;
use crate::admin::db::{ManagedBackendPatch, ManagedBackendRow};
use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// Masked view of a managed backend — never includes raw credentials.
#[derive(serde::Serialize)]
pub struct ManagedBackendResponse {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    /// true if api_key is Some and non-empty
    pub api_key_set: bool,
    /// true if both aws_access_key_id and aws_secret_access_key are set and non-empty
    pub aws_creds_set: bool,
    pub api_base: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub project: Option<String>,
    pub region: Option<String>,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

impl ManagedBackendResponse {
    pub fn from_row(row: &ManagedBackendRow) -> Self {
        let api_key_set = row.api_key.as_deref().is_some_and(|k| !k.is_empty());
        let aws_creds_set = row
            .aws_access_key_id
            .as_deref()
            .is_some_and(|k| !k.is_empty())
            && row
                .aws_secret_access_key
                .as_deref()
                .is_some_and(|k| !k.is_empty());
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            provider_id: row.provider_id.clone(),
            api_key_set,
            aws_creds_set,
            api_base: row.api_base.clone(),
            deployment: row.deployment.clone(),
            api_version: row.api_version.clone(),
            project: row.project.clone(),
            region: row.region.clone(),
            rpm: row.rpm,
            tpm: row.tpm,
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

/// Request body for POST /admin/api/backends/managed.
#[derive(serde::Deserialize)]
pub struct CreateManagedBackendRequest {
    pub name: String,
    pub provider_id: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub deployment: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub aws_access_key_id: Option<String>,
    #[serde(default)]
    pub aws_secret_access_key: Option<String>,
    #[serde(default)]
    pub aws_session_token: Option<String>,
    #[serde(default)]
    pub rpm: Option<u32>,
    #[serde(default)]
    pub tpm: Option<u64>,
}

/// GET /admin/api/backends/managed -- list all managed backends (masked).
pub async fn list(State(shared): State<SharedState>) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::list_managed_backends(conn)
    })
    .await;

    match result {
        Some(Ok(rows)) => {
            let backends: Vec<ManagedBackendResponse> =
                rows.iter().map(ManagedBackendResponse::from_row).collect();
            Json(serde_json::json!({ "backends": backends })).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to list managed backends"})),
        )
            .into_response(),
    }
}

/// POST /admin/api/backends/managed -- create a new managed backend.
pub async fn create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateManagedBackendRequest>,
) -> axum::response::Response {
    // 1. Validate name.
    if !crate::admin::routes::is_safe_model_name(&body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid backend name"})),
        )
            .into_response();
    }

    // 2. Validate provider.
    let provider = match shared.provider_catalog.get_provider(&body.provider_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Unknown provider_id"})),
            )
                .into_response()
        }
    };

    // 3. Build the row.
    let now = crate::admin::db::now_iso8601();
    let row = ManagedBackendRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name.clone(),
        provider_id: body.provider_id.clone(),
        api_key: body.api_key.clone(),
        api_base: body.api_base.clone(),
        deployment: body.deployment.clone(),
        api_version: body.api_version.clone(),
        project: body.project.clone(),
        region: body.region.clone(),
        aws_access_key_id: body.aws_access_key_id.clone(),
        aws_secret_access_key: body.aws_secret_access_key.clone(),
        aws_session_token: body.aws_session_token.clone(),
        rpm: body.rpm,
        tpm: body.tpm,
        created_at: now.clone(),
        updated_at: now,
    };

    // 4. Build BackendConfig and BackendClient (validates protocol is supported).
    let backend_config = match row_to_backend_config(&row, provider) {
        Ok(bc) => bc,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.message()})),
            )
                .into_response()
        }
    };
    let backend_client = crate::backend::BackendClient::from_backend_config(&backend_config);

    // 5. Insert into SQLite — 409 on duplicate name.
    let row_clone = row.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::insert_managed_backend(conn, &row_clone)
    })
    .await;

    match db_result {
        Some(Ok(())) => {}
        Some(Err(e)) if e.to_string().contains("UNIQUE constraint failed") => {
            return (
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({"error": "A managed backend with that name already exists"}),
                ),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to create managed backend"})),
            )
                .into_response();
        }
    }

    // 6. Insert into in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.insert(row.name.clone(), (row.clone(), backend_client));
    }

    // 7. Emit audit.
    crate::admin::routes::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_created".into(),
            target_type: "managed_backend".into(),
            target_id: Some(row.name.clone()),
            detail: Some(format!("provider_id={}", row.provider_id)),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    // 8. Return 201 with masked representation.
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "backend": ManagedBackendResponse::from_row(&row) })),
    )
        .into_response()
}

/// PUT /admin/api/backends/managed/{name} -- update a managed backend.
pub async fn update(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
    Json(patch): Json<ManagedBackendPatch>,
) -> axum::response::Response {
    // 1. Look up existing row from in-memory map.
    let existing_row = {
        let map = shared
            .managed_backends
            .read()
            .unwrap_or_else(|e| e.into_inner());
        map.get(&name).map(|(row, _)| row.clone())
    };

    let existing_row = match existing_row {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response()
        }
    };

    // 2. Apply patch to produce updated row.
    let mut updated_row = existing_row.clone();
    if let Some(v) = patch.provider_id.clone() {
        updated_row.provider_id = v;
    }
    if let Some(v) = patch.api_key.clone() {
        updated_row.api_key = Some(v);
    }
    if let Some(v) = patch.api_base.clone() {
        updated_row.api_base = Some(v);
    }
    if let Some(v) = patch.deployment.clone() {
        updated_row.deployment = Some(v);
    }
    if let Some(v) = patch.api_version.clone() {
        updated_row.api_version = Some(v);
    }
    if let Some(v) = patch.project.clone() {
        updated_row.project = Some(v);
    }
    if let Some(v) = patch.region.clone() {
        updated_row.region = Some(v);
    }
    if let Some(v) = patch.aws_access_key_id.clone() {
        updated_row.aws_access_key_id = Some(v);
    }
    if let Some(v) = patch.aws_secret_access_key.clone() {
        updated_row.aws_secret_access_key = Some(v);
    }
    if let Some(v) = patch.aws_session_token.clone() {
        updated_row.aws_session_token = Some(v);
    }
    if let Some(v) = patch.rpm {
        updated_row.rpm = Some(v);
    }
    if let Some(v) = patch.tpm {
        updated_row.tpm = Some(v);
    }

    // 3. Build BackendConfig/Client from updated row.
    let provider = match shared
        .provider_catalog
        .get_provider(&updated_row.provider_id)
    {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Unknown provider_id"})),
            )
                .into_response()
        }
    };

    let backend_config = match row_to_backend_config(&updated_row, provider) {
        Ok(bc) => bc,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.message()})),
            )
                .into_response()
        }
    };
    let new_client = crate::backend::BackendClient::from_backend_config(&backend_config);

    // 4. Update SQLite.
    let name_clone = name.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::update_managed_backend(conn, &name_clone, &patch)
    })
    .await;

    match db_result {
        Some(Ok(true)) => {}
        Some(Ok(false)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to update managed backend"})),
            )
                .into_response();
        }
    }

    // 5. Stamp updated_at before inserting into memory (SQLite already has the new value).
    updated_row.updated_at = crate::admin::db::now_iso8601();

    // 6. Update in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.insert(name.clone(), (updated_row.clone(), new_client));
    }

    // 7. Emit audit.
    crate::admin::routes::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_updated".into(),
            target_type: "managed_backend".into(),
            target_id: Some(name.clone()),
            detail: Some(format!("provider_id={}", updated_row.provider_id)),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({ "backend": ManagedBackendResponse::from_row(&updated_row) })),
    )
        .into_response()
}

/// DELETE /admin/api/backends/managed/{name} -- delete a managed backend.
pub async fn delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    // 1. Delete from SQLite.
    let name_clone = name.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_managed_backend(conn, &name_clone)
    })
    .await;

    match db_result {
        Some(Ok(true)) => {}
        Some(Ok(false)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete managed backend"})),
            )
                .into_response();
        }
    }

    // 2. Remove from in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.remove(&name);
    }

    // 3. Emit audit.
    crate::admin::routes::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_deleted".into(),
            target_type: "managed_backend".into(),
            target_id: Some(name.clone()),
            detail: None,
            source_ip: Some(addr.ip().to_string()),
        },
    );

    Json(serde_json::json!({ "status": "deleted", "name": name })).into_response()
}
