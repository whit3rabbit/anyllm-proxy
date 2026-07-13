use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// DELETE /admin/api/config/overrides/:key -- remove a single override.
pub(crate) async fn delete_config_override(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let key_clone = key.clone();
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_config_override(conn, &key_clone)
    })
    .await
    {
        Some(Ok(true)) => {
            let restored = match key.as_str() {
                "redact_secrets" => {
                    let env_default = shared.runtime_defaults.redact_secrets;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.redact_secrets = env_default;
                    }
                    Some(env_default.to_string())
                }
                "log_bodies" => {
                    let env_default = shared.runtime_defaults.log_bodies;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.log_bodies = env_default;
                    }
                    Some(env_default.to_string())
                }
                "anthropic_thinking_repair" => {
                    let env_default = shared.runtime_defaults.anthropic_thinking_repair;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.anthropic_thinking_repair = env_default;
                    }
                    Some(env_default.to_string())
                }
                "pxpipe_compress" => {
                    let env_default = shared.runtime_defaults.pxpipe_compress;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.pxpipe_compress = env_default;
                    }
                    Some(env_default.to_string())
                }
                "rtk_compress" => {
                    let env_default = shared.runtime_defaults.rtk_compress;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.rtk_compress = env_default;
                    }
                    Some(env_default.to_string())
                }
                "rtk_models" => {
                    let env_default = shared.runtime_defaults.rtk_models.clone();
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.rtk_models = env_default.clone();
                    }
                    Some(env_default)
                }
                "pxpipe_models" => {
                    let env_default = shared.runtime_defaults.pxpipe_models.clone();
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.pxpipe_models = env_default.clone();
                    }
                    Some(env_default)
                }
                "forward_client_auth" => {
                    let env_default = shared.runtime_defaults.forward_client_auth;
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.forward_client_auth = env_default;
                    }
                    Some(env_default.to_string())
                }
                "tool_guardrail_mode" => {
                    let env_default = shared.runtime_defaults.tool_guardrail_mode.clone();
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.tool_guardrail_mode = env_default.clone();
                    }
                    Some(env_default)
                }
                "optimizer_mode" => {
                    let env_default = shared.runtime_defaults.optimizer_mode.clone();
                    if let Ok(mut config) = shared.runtime_config.write() {
                        config.optimizer_mode = env_default.clone();
                    }
                    Some(env_default)
                }
                _ => None,
            };
            if let Some(value) = restored {
                let _ = shared
                    .events_tx
                    .send(crate::admin::state::AdminEvent::ConfigChanged {
                        key: key.clone(),
                        value,
                    });
            }
            super::super::emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "config_deleted".into(),
                    target_type: "config".into(),
                    target_id: Some(key.clone()),
                    detail: None,
                    source_ip: Some(addr.ip().to_string()),
                },
            );
            (StatusCode::OK, Json(serde_json::json!({"deleted": key}))).into_response()
        }
        Some(Ok(false)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "override not found"})),
        )
            .into_response(),
        Some(Err(e)) => {
            tracing::error!(error = %e, "delete_config_override failed");
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
