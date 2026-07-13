use anyllm_proxy::admin;
use anyllm_proxy::config;
use anyllm_proxy::server::state as server_state;
use std::path::Path;
use std::sync::Arc;

pub(crate) fn load_runtime_config(
    multi_config: &config::MultiConfig,
    tool_engine_state: &Option<Arc<server_state::ToolEngineState>>,
) -> (
    admin::state::RuntimeConfig,
    admin::state::RuntimeConfigDefaults,
) {
    let mut model_mappings = indexmap::IndexMap::new();
    for (name, bc) in &multi_config.backends {
        model_mappings.insert(name.clone(), bc.model_mapping.clone());
    }
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let tool_guardrail_default = tool_engine_state
        .as_ref()
        .map(|engine| engine.guardrails.mode.as_str().to_string())
        .unwrap_or_else(|| {
            anyllm_proxy::tools::ToolGuardrailMode::Disabled
                .as_str()
                .to_string()
        });

    let runtime_config = admin::state::RuntimeConfig {
        model_mappings,
        log_level,
        log_bodies: multi_config.log_bodies,
        redact_secrets: multi_config.redact_secrets,
        anthropic_thinking_repair: multi_config.anthropic_thinking_repair,
        pxpipe_compress: multi_config.pxpipe_compress,
        pxpipe_models: anyllm_proxy::pxpipe::resolve_default_models_csv(),
        rtk_compress: anyllm_proxy::rtk::resolve_default_enabled(),
        rtk_models: anyllm_proxy::rtk::resolve_default_models_csv(),
        forward_client_auth: multi_config.forward_client_auth,
        tool_guardrail_mode: tool_guardrail_default.clone(),
        optimizer_mode: anyllm_proxy::optimizer::resolve_default_mode()
            .as_str()
            .to_string(),
    };
    let runtime_defaults = admin::state::RuntimeConfigDefaults {
        log_bodies: multi_config.log_bodies,
        redact_secrets: multi_config.redact_secrets,
        anthropic_thinking_repair: multi_config.anthropic_thinking_repair,
        pxpipe_compress: multi_config.pxpipe_compress,
        pxpipe_models: anyllm_proxy::pxpipe::resolve_default_models_csv(),
        rtk_compress: anyllm_proxy::rtk::resolve_default_enabled(),
        rtk_models: anyllm_proxy::rtk::resolve_default_models_csv(),
        forward_client_auth: multi_config.forward_client_auth,
        tool_guardrail_mode: tool_guardrail_default,
        optimizer_mode: anyllm_proxy::optimizer::resolve_default_mode()
            .as_str()
            .to_string(),
    };
    (runtime_config, runtime_defaults)
}

pub(crate) fn apply_config_overrides(
    conn: &rusqlite::Connection,
    runtime_config: &mut admin::state::RuntimeConfig,
    multi_config: &config::MultiConfig,
) -> (bool, bool) {
    let mut log_bodies_enabled_by_override = false;
    let mut redact_secrets_enabled_by_override = false;

    if let Ok(overrides) = admin::db::get_config_overrides(conn) {
        for (key, value, _) in &overrides {
            match key.as_str() {
                "log_level" => {
                    const ALLOWED_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug"];
                    let normalized = value.trim().to_lowercase();
                    if ALLOWED_LOG_LEVELS.contains(&normalized.as_str()) {
                        runtime_config.log_level = normalized;
                    } else {
                        tracing::warn!(
                            value = %value,
                            "ignoring invalid log_level override from database"
                        );
                    }
                }
                "log_bodies" => {
                    runtime_config.log_bodies = value == "true";
                    log_bodies_enabled_by_override =
                        runtime_config.log_bodies && !multi_config.log_bodies;
                }
                "redact_secrets" => {
                    runtime_config.redact_secrets = value == "true";
                    redact_secrets_enabled_by_override =
                        runtime_config.redact_secrets && !multi_config.redact_secrets;
                }
                "anthropic_thinking_repair" => {
                    runtime_config.anthropic_thinking_repair = value == "true";
                }
                "pxpipe_compress" => {
                    runtime_config.pxpipe_compress = value == "true";
                }
                "pxpipe_models" => {
                    runtime_config.pxpipe_models = value.clone();
                }
                "rtk_compress" => {
                    runtime_config.rtk_compress = value == "true";
                }
                "rtk_models" => {
                    runtime_config.rtk_models = value.clone();
                }
                "forward_client_auth" => {
                    let wants_enabled = value == "true";
                    if wants_enabled
                        && anyllm_proxy::server::middleware::forward_client_auth_misconfigured(
                            anyllm_proxy::server::middleware::distinct_static_key_count(),
                            anyllm_proxy::server::middleware::open_relay_active(),
                        )
                    {
                        tracing::warn!(
                            "ignoring persisted forward_client_auth=true override: 2+ \
                             PROXY_API_KEYS entries with no PROXY_OPEN_RELAY would let \
                             different callers each redirect the upstream Anthropic credential"
                        );
                    } else {
                        runtime_config.forward_client_auth = wants_enabled;
                    }
                }
                "tool_guardrail_mode" => {
                    if value
                        .parse::<anyllm_proxy::tools::ToolGuardrailMode>()
                        .is_ok()
                    {
                        runtime_config.tool_guardrail_mode = value.clone();
                    } else {
                        tracing::warn!(
                            value = %value,
                            "ignoring invalid tool_guardrail_mode override from database"
                        );
                    }
                }
                "optimizer_mode" => {
                    if value.parse::<anyllm_optimize_core::Mode>().is_ok() {
                        runtime_config.optimizer_mode = value.clone();
                    } else {
                        tracing::warn!(
                            value = %value,
                            "ignoring invalid optimizer_mode override from database"
                        );
                    }
                }
                k if k.ends_with(".big_model") => {
                    let backend = k.strip_suffix(".big_model").unwrap();
                    if let Some(m) = runtime_config.model_mappings.get_mut(backend) {
                        m.big_model = value.clone();
                    }
                }
                k if k.ends_with(".small_model") => {
                    let backend = k.strip_suffix(".small_model").unwrap();
                    if let Some(m) = runtime_config.model_mappings.get_mut(backend) {
                        m.small_model = value.clone();
                    }
                }
                _ => {
                    tracing::debug!(key = %key, "unknown config override, skipping");
                }
            }
        }
        if !overrides.is_empty() {
            tracing::info!(
                count = overrides.len(),
                "applied config overrides from database"
            );
        }
    }
    (
        log_bodies_enabled_by_override,
        redact_secrets_enabled_by_override,
    )
}

pub(crate) fn load_virtual_keys(
    conn: &rusqlite::Connection,
) -> Arc<dashmap::DashMap<[u8; 32], admin::keys::VirtualKeyMeta>> {
    let virtual_keys = Arc::new(dashmap::DashMap::new());
    if let Ok(active_keys) = admin::db::load_active_virtual_keys(conn) {
        for key_row in &active_keys {
            if let Some(hash_bytes) = admin::keys::hash_from_hex(&key_row.key_hash) {
                virtual_keys.insert(
                    hash_bytes,
                    admin::keys::VirtualKeyMeta {
                        id: key_row.id,
                        description: key_row.description.clone(),
                        expires_at: key_row.expires_at.as_deref().and_then(|s| {
                            anyllm_proxy::integrations::langfuse::iso8601_to_epoch(s)
                                .and_then(|e| i64::try_from(e).ok())
                        }),
                        rpm_limit: key_row.rpm_limit,
                        tpm_limit: key_row.tpm_limit,
                        rate_state: Arc::new(admin::keys::RateLimitState::new()),
                        role: admin::keys::KeyRole::from_str_or_default(&key_row.role),
                        max_budget_usd: key_row.max_budget_usd,
                        budget_duration: key_row
                            .budget_duration
                            .as_deref()
                            .and_then(admin::keys::BudgetDuration::parse),
                        period_start: key_row.period_start.clone(),
                        period_spend_usd: key_row.period_spend_usd,
                        allowed_models: key_row.allowed_models.clone(),
                        allowed_routes: key_row.allowed_routes.clone(),
                    },
                );
            }
        }
        tracing::info!(
            count = active_keys.len(),
            "loaded virtual API keys from database"
        );
    }
    virtual_keys
}

pub(crate) fn load_managed_backends(
    conn: &rusqlite::Connection,
    provider_catalog: &Arc<anyllm_providers::ProviderCatalog>,
) -> Arc<
    std::sync::RwLock<
        std::collections::HashMap<
            String,
            (
                anyllm_proxy::admin::db::ManagedBackendRow,
                anyllm_proxy::backend::BackendClient,
            ),
        >,
    >,
> {
    let mut map = std::collections::HashMap::new();
    if let Ok(rows) = admin::db::list_managed_backends(conn) {
        for row in rows {
            match provider_catalog.get_provider(&row.provider_id) {
                None => {
                    tracing::warn!(
                        provider_id = %row.provider_id,
                        backend_id = %row.id,
                        "managed backend references unknown provider; skipping"
                    );
                }
                Some(provider) => {
                    match anyllm_proxy::admin::routes::managed_backends::row_to_backend_config(
                        &row, provider,
                    ) {
                        Err(e) => {
                            tracing::warn!(
                                provider_id = %row.provider_id,
                                backend_id = %row.id,
                                error = %e.message(),
                                "managed backend configuration is invalid; skipping"
                            );
                        }
                        Ok(bc) => {
                            let client =
                                anyllm_proxy::backend::BackendClient::from_backend_config(&bc);
                            map.insert(row.name.clone(), (row, client));
                        }
                    }
                }
            }
        }
    }
    tracing::info!(count = map.len(), "loaded managed backends from SQLite");
    Arc::new(std::sync::RwLock::new(map))
}

pub(crate) fn resolve_admin_token(data_dir: &Path) -> (String, Arc<zeroize::Zeroizing<String>>) {
    let admin_token = match std::env::var("ADMIN_TOKEN") {
        Ok(t) => {
            if t.len() < 32 {
                tracing::warn!(
                    len = t.len(),
                    "ADMIN_TOKEN is shorter than 32 characters; use a longer random value to reduce brute-force risk (generate one with: openssl rand -hex 32)"
                );
            }
            t
        }
        Err(_) => {
            let mut buf = [0u8; 32];
            getrandom::fill(&mut buf).expect("getrandom failed");
            let token = hex::encode(buf);
            let token_path = crate::main_helpers::bootstrap::resolve_admin_token_path(data_dir);
            let token_path_str = token_path.to_string_lossy().to_string();
            if let Err(e) =
                crate::main_helpers::bootstrap::write_token_file(&token_path_str, &token)
            {
                panic!(
                    "Cannot write admin token to {token_path_str}: {e}. Set ADMIN_TOKEN env var explicitly or ensure the path is writable."
                );
            } else {
                tracing::info!(
                    path = %token_path_str,
                    "admin token written to {token_path_str}; set ADMIN_TOKEN for a fixed token across restarts"
                );
            }
            token
        }
    };
    let admin_token_plain = admin_token.clone();
    let admin_token_wrapped = Arc::new(zeroize::Zeroizing::new(admin_token));
    (admin_token_plain, admin_token_wrapped)
}
