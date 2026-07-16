use crate::admin::state::SharedState;
use axum::{extract::State, Json};

/// GET /admin/api/config -- effective config (env defaults + overrides).
pub(crate) async fn get_config(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let (
        log_level,
        log_bodies,
        redact_secrets,
        anthropic_thinking_repair,
        pxpipe_compress,
        pxpipe_models,
        rtk_compress,
        rtk_models,
        forward_client_auth,
        tool_guardrail_mode,
        optimizer_mode,
        router,
        backends,
    ) = {
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
        (
            config.log_level.clone(),
            config.log_bodies,
            config.redact_secrets,
            config.anthropic_thinking_repair,
            config.pxpipe_compress,
            config.pxpipe_models.clone(),
            config.rtk_compress,
            config.rtk_models.clone(),
            config.forward_client_auth,
            config.tool_guardrail_mode.clone(),
            config.optimizer_mode.clone(),
            config.router.clone(),
            backends,
        )
    };

    let overrides = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::get_config_overrides(conn).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let override_keys: Vec<String> = overrides.iter().map(|(k, _, _)| k.clone()).collect();

    let pxpipe_available_models =
        crate::pxpipe::available_vision_models(&shared.provider_catalog, &pxpipe_models);

    Json(serde_json::json!({
        "log_level": log_level,
        "log_bodies": log_bodies,
        "redact_secrets": redact_secrets,
        "anthropic_thinking_repair": anthropic_thinking_repair,
        "pxpipe_compress": pxpipe_compress,
        "pxpipe_models": pxpipe_models,
        "pxpipe_available_models": pxpipe_available_models,
        "rtk_compress": rtk_compress,
        "rtk_models": rtk_models,
        "forward_client_auth": forward_client_auth,
        "tool_guardrail_mode": tool_guardrail_mode,
        "optimizer_mode": optimizer_mode,
        "router": router,
        "backends": backends,
        "overridden_keys": override_keys,
    }))
}

/// GET /admin/api/config/overrides -- only SQLite overrides.
pub(crate) async fn get_config_overrides(
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
