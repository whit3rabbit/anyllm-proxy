use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

#[derive(serde::Deserialize)]
pub(super) struct CreateKeyRequest {
    description: Option<String>,
    expires_at: Option<String>,
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    spend_limit: Option<f64>,
    role: Option<String>,
    max_budget_usd: Option<f64>,
    budget_duration: Option<String>,
    allowed_models: Option<Vec<String>>,
}

/// POST /admin/api/keys -- create a new virtual API key.
pub(super) async fn create_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {
    let (raw_key, key_prefix, key_hash_hex) =
        super::super::keys::generate_virtual_key(&shared.hmac_secret);
    let role_str = body.role.as_deref().unwrap_or("developer");
    let role = super::super::keys::KeyRole::from_str_or_default(role_str);
    let result = super::super::state::with_db(&shared.db, {
        let hash = key_hash_hex.clone();
        let prefix = key_prefix.clone();
        let desc = body.description.clone();
        let exp = body.expires_at.clone();
        let rpm = body.rpm_limit;
        let tpm = body.tpm_limit;
        let spend = body.spend_limit;
        let role_s = role_str.to_string();
        let max_budget = body.max_budget_usd;
        let budget_dur = body.budget_duration.clone();
        let allowed_models_json = body
            .allowed_models
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        move |conn| {
            super::super::db::insert_virtual_key(
                conn,
                &super::super::db::InsertVirtualKeyParams {
                    key_hash: &hash,
                    key_prefix: &prefix,
                    description: desc.as_deref(),
                    expires_at: exp.as_deref(),
                    rpm_limit: rpm,
                    tpm_limit: tpm,
                    spend_limit: spend,
                    role: &role_s,
                    max_budget_usd: max_budget,
                    budget_duration: budget_dur.as_deref(),
                    allowed_models: allowed_models_json,
                },
            )
        }
    })
    .await;

    match result {
        Some(Ok(id)) => {
            if let Some(hash_bytes) = super::super::keys::hash_from_hex(&key_hash_hex) {
                shared.virtual_keys.insert(
                    hash_bytes,
                    super::super::keys::VirtualKeyMeta {
                        id,
                        description: body.description.clone(),
                        expires_at: body.expires_at.as_deref().and_then(|s| {
                            crate::integrations::langfuse::iso8601_to_epoch(s)
                                .and_then(|e| i64::try_from(e).ok())
                        }),
                        rpm_limit: body.rpm_limit,
                        tpm_limit: body.tpm_limit,
                        rate_state: std::sync::Arc::new(
                            super::super::keys::RateLimitState::new(),
                        ),
                        role,
                        max_budget_usd: body.max_budget_usd,
                        budget_duration: body
                            .budget_duration
                            .as_deref()
                            .and_then(super::super::keys::BudgetDuration::parse),
                        period_start: Some(super::super::db::now_iso8601()),
                        period_spend_usd: 0.0,
                        allowed_models: body.allowed_models.clone(),
                    },
                );
            }
            super::emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_created".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: Some(format!(
                        "description={}, prefix={}",
                        body.description.as_deref().unwrap_or(""),
                        key_prefix
                    )),
                    source_ip: Some(addr.ip().to_string()),
                },
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": id,
                    "key": raw_key,
                    "key_prefix": key_prefix,
                    "description": body.description,
                    "created_at": super::super::db::now_iso8601(),
                    "expires_at": body.expires_at,
                    "rpm_limit": body.rpm_limit,
                    "tpm_limit": body.tpm_limit,
                    "spend_limit": body.spend_limit,
                    "role": role.as_str(),
                    "max_budget_usd": body.max_budget_usd,
                    "budget_duration": body.budget_duration,
                    "allowed_models": body.allowed_models,
                })),
            )
                .into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to create key"})),
        )
            .into_response(),
    }
}

/// GET /admin/api/keys -- list all virtual keys.
pub(super) async fn list_keys(
    State(shared): State<SharedState>,
) -> axum::response::Response {
    let result =
        super::super::state::with_db(&shared.db, super::super::db::list_virtual_keys).await;
    match result {
        Some(Ok(keys)) => {
            let enriched: Vec<serde_json::Value> = keys
                .iter()
                .map(|k| {
                    serde_json::json!({
                        "id": k.id,
                        "key_prefix": k.key_prefix,
                        "description": k.description,
                        "created_at": k.created_at,
                        "expires_at": k.expires_at,
                        "revoked_at": k.revoked_at,
                        "rpm_limit": k.rpm_limit,
                        "tpm_limit": k.tpm_limit,
                        "spend_limit": k.spend_limit,
                        "total_spend": k.total_spend,
                        "total_requests": k.total_requests,
                        "total_tokens": k.total_tokens,
                        "status": k.status(),
                        "role": k.role,
                        "max_budget_usd": k.max_budget_usd,
                        "budget_duration": k.budget_duration,
                        "period_spend_usd": k.period_spend_usd,
                        "period_reset_at": crate::admin::keys::period_reset_at_from_row(k),
                        "allowed_models": k.allowed_models,
                    })
                })
                .collect();
            Json(serde_json::json!({ "keys": enriched })).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to list keys"})),
        )
            .into_response(),
    }
}

/// Request body for PUT /admin/api/keys/{id}.
/// All fields are optional: absent = clear (set to NULL); role is immutable after creation.
#[derive(serde::Deserialize)]
pub(super) struct UpdateKeyRequest {
    description: Option<String>,
    expires_at: Option<String>,
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    max_budget_usd: Option<f64>,
    budget_duration: Option<String>,
    allowed_models: Option<Vec<String>>,
}

/// PUT /admin/api/keys/{id} -- update an existing virtual key (except role).
pub(super) async fn update_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateKeyRequest>,
) -> axum::response::Response {
    let allowed_models_json = body
        .allowed_models
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let desc = body.description.clone();
    let exp = body.expires_at.clone();
    let rpm = body.rpm_limit;
    let tpm = body.tpm_limit;
    let max_budget = body.max_budget_usd;
    let budget_dur = body.budget_duration.clone();

    let result = super::super::state::with_db(&shared.db, move |conn| {
        super::super::db::update_virtual_key(
            conn,
            id,
            &super::super::db::UpdateVirtualKeyParams {
                description: desc.as_deref(),
                expires_at: exp.as_deref(),
                rpm_limit: rpm,
                tpm_limit: tpm,
                max_budget_usd: max_budget,
                budget_duration: budget_dur.as_deref(),
                allowed_models: allowed_models_json,
            },
        )
    })
    .await;

    match result {
        Some(Ok(Some(row))) => {
            // Refresh the DashMap entry so in-flight auth sees updated limits.
            if let Some(hash_bytes) = super::super::keys::hash_from_hex(&row.key_hash) {
                shared.virtual_keys.entry(hash_bytes).and_modify(|meta| {
                    meta.description = body.description.clone();
                    meta.expires_at = body.expires_at.as_deref().and_then(|s| {
                        crate::integrations::langfuse::iso8601_to_epoch(s)
                            .and_then(|e| i64::try_from(e).ok())
                    });
                    meta.rpm_limit = body.rpm_limit;
                    meta.tpm_limit = body.tpm_limit;
                    meta.max_budget_usd = body.max_budget_usd;
                    if body.budget_duration.is_some() {
                        meta.budget_duration = body
                            .budget_duration
                            .as_deref()
                            .and_then(super::super::keys::BudgetDuration::parse);
                        // Reset spend period to match db-layer reset.
                        meta.period_start = None;
                        meta.period_spend_usd = 0.0;
                    }
                    meta.allowed_models = body.allowed_models.clone();
                });
            }
            super::emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_updated".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: Some(format!("prefix={}", row.key_prefix)),
                    source_ip: Some(addr.ip().to_string()),
                },
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": row.id,
                    "key_prefix": row.key_prefix,
                    "description": row.description,
                    "expires_at": row.expires_at,
                    "rpm_limit": row.rpm_limit,
                    "tpm_limit": row.tpm_limit,
                    "role": row.role,
                    "max_budget_usd": row.max_budget_usd,
                    "budget_duration": row.budget_duration,
                    "allowed_models": row.allowed_models,
                    "status": row.status(),
                })),
            )
                .into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found or already revoked"})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to update key"})),
        )
            .into_response(),
    }
}

/// DELETE /admin/api/keys/{id} -- revoke a virtual key.
pub(super) async fn revoke_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> axum::response::Response {
    let result = super::super::state::with_db(&shared.db, move |conn| {
        super::super::db::revoke_virtual_key(conn, id)
    })
    .await;
    match result {
        Some(Ok(Some(row))) => {
            if let Some(hash_bytes) = super::super::keys::hash_from_hex(&row.key_hash) {
                shared.virtual_keys.remove(&hash_bytes);
            }
            super::emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_revoked".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: None,
                    source_ip: Some(addr.ip().to_string()),
                },
            );
            Json(serde_json::json!({
                "id": row.id,
                "revoked_at": row.revoked_at,
                "status": "revoked",
            }))
            .into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found or already revoked"})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to revoke key"})),
        )
            .into_response(),
    }
}
