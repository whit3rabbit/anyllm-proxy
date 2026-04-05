use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// GET /admin/api/models -- list all routed model names and deployment counts.
pub(super) async fn list_models(State(shared): State<SharedState>) -> impl IntoResponse {
    if let Some(ref router_lock) = shared.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        let models: Vec<serde_json::Value> = router
            .list_models()
            .into_iter()
            .map(|(name, count)| {
                serde_json::json!({
                    "model_name": name,
                    "deployments": count,
                })
            })
            .collect();
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": format!("{:?}", router.strategy()),
                "models": models,
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": null,
                "models": [],
                "note": "no model router active (not using LiteLLM config)"
            })),
        )
            .into_response()
    }
}

/// Request body for POST /admin/api/models.
#[derive(serde::Deserialize)]
pub(super) struct AddModelRequest {
    model_name: String,
    backend_name: String,
    actual_model: String,
    #[serde(default)]
    rpm: Option<u32>,
    #[serde(default)]
    tpm: Option<u64>,
    #[serde(default = "default_weight")]
    weight: u32,
}

pub(super) fn default_weight() -> u32 {
    1
}

/// POST /admin/api/models -- add a deployment for a model name.
pub(super) async fn add_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    // Validate name fields to prevent log injection via control characters.
    for (field, value) in [
        ("model_name", &body.model_name),
        ("backend_name", &body.backend_name),
        ("actual_model", &body.actual_model),
    ] {
        if !super::is_safe_model_name(value) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid {field}: contains disallowed characters")
                })),
            )
                .into_response();
        }
    }

    // Validate that backend_name refers to a configured backend.
    if !shared.backend_metrics.contains_key(&body.backend_name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unknown backend: {}", body.backend_name)
            })),
        )
            .into_response();
    }

    let deployment = std::sync::Arc::new(crate::config::model_router::Deployment::with_weight(
        body.backend_name.clone(),
        body.actual_model.clone(),
        body.rpm,
        body.tpm,
        body.weight,
    ));

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    router.add_deployment(body.model_name.clone(), deployment);

    tracing::info!(
        model_name = %body.model_name,
        backend = %body.backend_name,
        actual_model = %body.actual_model,
        "added model deployment via admin API"
    );

    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "model_added".into(),
            target_type: "model".into(),
            target_id: Some(body.model_name.clone()),
            detail: Some(format!(
                "backend={}, actual_model={}",
                body.backend_name, body.actual_model
            )),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "added",
            "model_name": body.model_name,
            "backend_name": body.backend_name,
            "actual_model": body.actual_model,
        })),
    )
        .into_response()
}

/// DELETE /admin/api/models/{name} -- remove all deployments for a model.
pub(super) async fn remove_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    if router.remove_model(&name) {
        tracing::info!(model_name = %name, "removed model via admin API");
        super::emit_audit(
            &shared,
            crate::admin::db::AuditEntry {
                id: None,
                timestamp: None,
                action: "model_removed".into(),
                target_type: "model".into(),
                target_id: Some(name.clone()),
                detail: None,
                source_ip: Some(addr.ip().to_string()),
            },
        );
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "removed", "model_name": name})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "model not found", "model_name": name})),
        )
            .into_response()
    }
}
