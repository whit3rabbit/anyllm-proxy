use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// GET /admin/api/env -- effective environment variable values.
/// Secrets (API keys, tokens) are masked; plain config values are shown as-is.
pub(super) async fn get_env() -> Json<serde_json::Value> {
    fn plain(key: &str) -> serde_json::Value {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => serde_json::Value::String(v),
            _ => serde_json::Value::Null,
        }
    }
    fn secret(key: &str) -> serde_json::Value {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => {
                serde_json::Value::String(anyllm_translate::util::redact::redact_secret(&v))
            }
            _ => serde_json::Value::Null,
        }
    }

    Json(serde_json::json!({
        // Core proxy config
        "BACKEND":            plain("BACKEND"),
        "LISTEN_PORT":        plain("LISTEN_PORT"),
        "BIG_MODEL":          plain("BIG_MODEL"),
        "SMALL_MODEL":        plain("SMALL_MODEL"),
        "RUST_LOG":           plain("RUST_LOG"),
        "LOG_BODIES":         plain("LOG_BODIES"),
        "PROXY_CONFIG":       plain("PROXY_CONFIG"),
        // OpenAI / compatible
        "OPENAI_BASE_URL":    plain("OPENAI_BASE_URL"),
        "OPENAI_API_FORMAT":  plain("OPENAI_API_FORMAT"),
        "OPENAI_API_KEY":     secret("OPENAI_API_KEY"),
        // Vertex AI
        "VERTEX_PROJECT":     plain("VERTEX_PROJECT"),
        "VERTEX_REGION":      plain("VERTEX_REGION"),
        "VERTEX_API_KEY":     secret("VERTEX_API_KEY"),
        // Gemini
        "GEMINI_BASE_URL":    plain("GEMINI_BASE_URL"),
        "GEMINI_API_KEY":     secret("GEMINI_API_KEY"),
        // Azure OpenAI
        "AZURE_OPENAI_ENDPOINT":    plain("AZURE_OPENAI_ENDPOINT"),
        "AZURE_OPENAI_DEPLOYMENT":  plain("AZURE_OPENAI_DEPLOYMENT"),
        "AZURE_OPENAI_API_KEY":     secret("AZURE_OPENAI_API_KEY"),
        "AZURE_OPENAI_API_VERSION": plain("AZURE_OPENAI_API_VERSION"),
        // AWS Bedrock
        "AWS_REGION":               plain("AWS_REGION"),
        "AWS_ACCESS_KEY_ID":        secret("AWS_ACCESS_KEY_ID"),
        "AWS_SECRET_ACCESS_KEY":    secret("AWS_SECRET_ACCESS_KEY"),
        "AWS_SESSION_TOKEN":        secret("AWS_SESSION_TOKEN"),
        // Google OAuth bearer token (full token — treat as secret)
        "GOOGLE_ACCESS_TOKEN":      secret("GOOGLE_ACCESS_TOKEN"),
        // Auth
        "PROXY_API_KEYS":     secret("PROXY_API_KEYS"),
        "PROXY_OPEN_RELAY":   plain("PROXY_OPEN_RELAY"),
        // TLS
        "TLS_CLIENT_CERT_P12": plain("TLS_CLIENT_CERT_P12"),
        "TLS_CA_CERT":         plain("TLS_CA_CERT"),
        // Network / security
        "IP_ALLOWLIST":           plain("IP_ALLOWLIST"),
        "TRUST_PROXY_HEADERS":    plain("TRUST_PROXY_HEADERS"),
        "WEBHOOK_URLS":           plain("WEBHOOK_URLS"),
        "RATE_LIMIT_FAIL_POLICY": plain("RATE_LIMIT_FAIL_POLICY"),
        // Admin
        "ADMIN_PORT":               plain("ADMIN_PORT"),
        "ADMIN_DB_PATH":            plain("ADMIN_DB_PATH"),
        "ADMIN_LOG_RETENTION_DAYS": plain("ADMIN_LOG_RETENTION_DAYS"),
    }))
}

/// GET /admin/api/config -- effective config (env defaults + overrides).
pub(super) async fn get_config(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    // Clone config snapshot and drop the read guard before any .await points.
    // std::sync::RwLockReadGuard is !Send, cannot be held across awaits.
    let (log_level, log_bodies, backends) = {
        let config = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let mut backends = serde_json::Map::new();
        for (name, mapping) in &config.model_mappings {
            backends.insert(
                name.clone(),
                serde_json::json!({
                    "big_model": mapping.big_model,
                    "small_model": mapping.small_model,
                }),
            );
        }
        (config.log_level.clone(), config.log_bodies, backends)
    };

    // Get overrides to mark which fields are overridden.
    let overrides = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::get_config_overrides(conn).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let override_keys: Vec<String> = overrides.iter().map(|(k, _, _)| k.clone()).collect();

    Json(serde_json::json!({
        "log_level": log_level,
        "log_bodies": log_bodies,
        "backends": backends,
        "overridden_keys": override_keys,
    }))
}

/// PUT /admin/api/config -- update config overrides. Partial JSON body.
pub(super) async fn put_config(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Collect the key-value pairs to persist, then do all SQLite I/O
    // before touching in-memory state. This avoids holding the async
    // MutexGuard across block_in_place.
    let mut db_writes: Vec<(String, String)> = Vec::new();

    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        // Allowlist: trace-level logging exposes HTTP headers (including API
        // keys) in log output. Arbitrary filter directives could also be used
        // to selectively leak data. Restrict to safe levels only.
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
    if let Some(backends) = body.get("backends").and_then(|v| v.as_object()) {
        // Read current config to validate backend names exist
        let config = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for (name, settings) in backends {
            if config.model_mappings.contains_key(name) {
                if let Some(big) = settings.get("big_model").and_then(|v| v.as_str()) {
                    if !super::is_safe_model_name(big) {
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
                    if !super::is_safe_model_name(small) {
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

    // Serialize config writes so concurrent requests cannot interleave
    // Phase 1 (SQLite) and Phase 2 (in-memory), which would leave them
    // inconsistent.
    let _config_guard = shared.config_write_lock.lock().await;

    // Phase 1: Persist to SQLite first. If the process crashes between
    // phases, the database is the source of truth and config is restored
    // on restart. Reversing the order would lose updates on crash.
    {
        let writes = db_writes.clone();
        crate::admin::state::with_db(&shared.db, move |conn| {
            for (key, value) in &writes {
                crate::admin::db::set_config_override(conn, key, value).ok();
            }
        })
        .await;
    }

    // Phase 2: Apply to in-memory config (no async lock held)
    {
        let mut config = shared
            .runtime_config
            .write()
            .unwrap_or_else(|e| e.into_inner());

        // Audit log: capture old values before applying changes
        for (key, new_value) in &db_writes {
            let old_value = match key.as_str() {
                "log_level" => config.log_level.clone(),
                "log_bodies" => config.log_bodies.to_string(),
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

    // Broadcast config changes.
    for (key, value) in &db_writes {
        let _ = shared
            .events_tx
            .send(crate::admin::state::AdminEvent::ConfigChanged {
                key: key.clone(),
                value: value.clone(),
            });
        super::emit_audit(
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

/// GET /admin/api/config/overrides -- only SQLite overrides.
pub(super) async fn get_config_overrides(
    State(shared): State<SharedState>,
) -> Json<serde_json::Value> {
    let overrides = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::get_config_overrides(conn).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    let entries: Vec<serde_json::Value> = overrides
        .into_iter()
        .map(|(k, v, updated_at)| {
            serde_json::json!({
                "key": k,
                "value": v,
                "updated_at": updated_at,
            })
        })
        .collect();

    Json(serde_json::json!({ "overrides": entries }))
}

/// DELETE /admin/api/config/overrides/:key -- remove a single override.
pub(super) async fn delete_config_override(
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
            super::emit_audit(
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
