use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// PUT /admin/api/config -- update config overrides. Partial JSON body.
pub(crate) async fn put_config(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut db_writes: Vec<(String, String)> = Vec::new();

    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        const ALLOWED_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug"];
        let normalized = level.trim().to_lowercase();
        if !ALLOWED_LOG_LEVELS.contains(&normalized.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "invalid log_level '{}': allowed values are {:?}. \
                         Set RUST_LOG at startup for advanced filter directives.",
                        level, ALLOWED_LOG_LEVELS
                    )
                })),
            )
                .into_response();
        }
        db_writes.push(("log_level".to_string(), normalized));
    }
    if let Some(val) = body.get("log_bodies").and_then(|v| v.as_bool()) {
        if val {
            tracing::warn!(
                "admin API: log_bodies enabled -- request/response bodies will be logged, \
                 which may include sensitive data (PII, API keys in forwarded requests)"
            );
        }
        db_writes.push(("log_bodies".to_string(), val.to_string()));
    }
    if let Some(val) = body.get("redact_secrets").and_then(|v| v.as_bool()) {
        if let Err(message) = crate::server::secret_redaction::ensure_available(val) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message })),
            )
                .into_response();
        }
        if val {
            tracing::warn!(
                "admin API: redact_secrets enabled -- upstream JSON/text request payloads will \
                 be scanned and detected secrets replaced before forwarding"
            );
        }
        db_writes.push(("redact_secrets".to_string(), val.to_string()));
    }
    if let Some(val) = body
        .get("anthropic_thinking_repair")
        .and_then(|v| v.as_bool())
    {
        db_writes.push(("anthropic_thinking_repair".to_string(), val.to_string()));
    }
    if let Some(val) = body.get("pxpipe_compress").and_then(|v| v.as_bool()) {
        db_writes.push(("pxpipe_compress".to_string(), val.to_string()));
    }
    if let Some(val) = body.get("pxpipe_models").and_then(|v| v.as_str()) {
        let normalized = crate::config::helpers::normalize_csv(val);
        db_writes.push(("pxpipe_models".to_string(), normalized));
    }
    if let Some(val) = body.get("rtk_compress").and_then(|v| v.as_bool()) {
        db_writes.push(("rtk_compress".to_string(), val.to_string()));
    }
    if let Some(val) = body.get("rtk_models").and_then(|v| v.as_str()) {
        let normalized = crate::config::helpers::normalize_csv(val);
        db_writes.push(("rtk_models".to_string(), normalized));
    }
    if let Some(val) = body.get("forward_client_auth").and_then(|v| v.as_bool()) {
        if val
            && crate::server::middleware::forward_client_auth_misconfigured(
                crate::server::middleware::distinct_static_key_count(),
                crate::server::middleware::open_relay_active(),
            )
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "ANTHROPIC_FORWARD_CLIENT_AUTH cannot be enabled with 2+ \
                              PROXY_API_KEYS entries and no PROXY_OPEN_RELAY: this would let \
                              different callers each redirect the upstream Anthropic \
                              credential. Use exactly one PROXY_API_KEYS entry or \
                              PROXY_OPEN_RELAY=true for a single-operator/BYOK deployment."
                })),
            )
                .into_response();
        }
        db_writes.push(("forward_client_auth".to_string(), val.to_string()));
    }
    if let Some(val) = body.get("tool_guardrail_mode").and_then(|v| v.as_str()) {
        match val.parse::<crate::tools::ToolGuardrailMode>() {
            Ok(mode) => {
                db_writes.push(("tool_guardrail_mode".to_string(), mode.as_str().to_string()))
            }
            Err(message) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": message })),
                )
                    .into_response();
            }
        }
    }
    if let Some(val) = body.get("optimizer_mode").and_then(|v| v.as_str()) {
        match val.parse::<anyllm_optimize_core::Mode>() {
            Ok(mode) => db_writes.push(("optimizer_mode".to_string(), mode.as_str().to_string())),
            Err(message) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": message })),
                )
                    .into_response();
            }
        }
    }
    if let Some(backends) = body.get("backends").and_then(|v| v.as_object()) {
        let config = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for (name, settings) in backends {
            if config.model_mappings.contains_key(name) {
                if let Some(big) = settings.get("big_model").and_then(|v| v.as_str()) {
                    if !super::super::is_safe_model_name(big) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": format!("invalid big_model name '{big}': contains disallowed characters")
                            })),
                        )
                            .into_response();
                    }
                    db_writes.push((format!("{name}.big_model"), big.to_string()));
                }
                if let Some(small) = settings.get("small_model").and_then(|v| v.as_str()) {
                    if !super::super::is_safe_model_name(small) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": format!("invalid small_model name '{small}': contains disallowed characters")
                            })),
                        )
                            .into_response();
                    }
                    db_writes.push((format!("{name}.small_model"), small.to_string()));
                }
            }
        }
    }

    let _config_guard = shared.config_write_lock.lock().await;

    {
        let writes = db_writes.clone();
        crate::admin::state::with_db(&shared.db, move |conn| {
            for (key, value) in &writes {
                crate::admin::db::set_config_override(conn, key, value).ok();
            }
        })
        .await;
    }

    {
        let mut config = shared
            .runtime_config
            .write()
            .unwrap_or_else(|e| e.into_inner());

        for (key, new_value) in &db_writes {
            let old_value = match key.as_str() {
                "log_level" => config.log_level.clone(),
                "log_bodies" => config.log_bodies.to_string(),
                "redact_secrets" => config.redact_secrets.to_string(),
                "anthropic_thinking_repair" => config.anthropic_thinking_repair.to_string(),
                "pxpipe_compress" => config.pxpipe_compress.to_string(),
                "pxpipe_models" => config.pxpipe_models.clone(),
                "rtk_compress" => config.rtk_compress.to_string(),
                "rtk_models" => config.rtk_models.clone(),
                "forward_client_auth" => config.forward_client_auth.to_string(),
                "tool_guardrail_mode" => config.tool_guardrail_mode.clone(),
                "optimizer_mode" => config.optimizer_mode.clone(),
                other => {
                    if let Some((backend, field)) = other.split_once('.') {
                        config
                            .model_mappings
                            .get(backend)
                            .map(|m| match field {
                                "big_model" => m.big_model.clone(),
                                "small_model" => m.small_model.clone(),
                                _ => "<unknown>".to_string(),
                            })
                            .unwrap_or_else(|| "<unset>".to_string())
                    } else {
                        "<unknown>".to_string()
                    }
                }
            };
            tracing::info!(
                key = %key,
                old_value = %old_value,
                new_value = %new_value,
                "admin config change"
            );
        }

        for (key, value) in &db_writes {
            match key.as_str() {
                "log_level" => {
                    config.log_level = value.clone();
                    if let Some(ref reload) = shared.log_reload {
                        if !reload(value) {
                            tracing::warn!(filter = value, "failed to apply log level change");
                        }
                    }
                }
                "log_bodies" => {
                    config.log_bodies = value == "true";
                }
                "redact_secrets" => {
                    config.redact_secrets = value == "true";
                }
                "anthropic_thinking_repair" => {
                    config.anthropic_thinking_repair = value == "true";
                }
                "pxpipe_compress" => {
                    config.pxpipe_compress = value == "true";
                }
                "pxpipe_models" => {
                    config.pxpipe_models = value.clone();
                }
                "rtk_compress" => {
                    config.rtk_compress = value == "true";
                }
                "rtk_models" => {
                    config.rtk_models = value.clone();
                }
                "forward_client_auth" => {
                    config.forward_client_auth = value == "true";
                }
                "tool_guardrail_mode" => {
                    config.tool_guardrail_mode = value.clone();
                }
                "optimizer_mode" => {
                    config.optimizer_mode = value.clone();
                }
                _ => {
                    if let Some((backend, field)) = key.split_once('.') {
                        if let Some(mapping) = config.model_mappings.get_mut(backend) {
                            match field {
                                "big_model" => mapping.big_model = value.clone(),
                                "small_model" => mapping.small_model = value.clone(),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    drop(_config_guard);

    for (key, value) in &db_writes {
        let _ = shared
            .events_tx
            .send(crate::admin::state::AdminEvent::ConfigChanged {
                key: key.clone(),
                value: value.clone(),
            });
        super::super::emit_audit(
            &shared,
            crate::admin::db::AuditEntry {
                id: None,
                timestamp: None,
                action: "config_changed".into(),
                target_type: "config".into(),
                target_id: Some(key.clone()),
                detail: Some(format!("value={value}")),
                source_ip: Some(addr.ip().to_string()),
            },
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "updated": db_writes.len(),
            "keys": db_writes.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        })),
    )
        .into_response()
}
