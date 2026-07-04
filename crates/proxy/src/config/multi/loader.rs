use super::{BackendConfig, MultiConfig};
use crate::config::single::{bedrock_credentials_from_env, Config};
use crate::config::types::BackendKind;
use indexmap::IndexMap;
use std::sync::Arc;

/// Result of `MultiConfig::load()`.
pub struct LoadResult {
    pub multi_config: MultiConfig,
    pub model_router: Option<Arc<std::sync::RwLock<crate::config::model_router::ModelRouter>>>,
    /// Resolved master_key from LiteLLM general_settings, if present.
    /// Caller should apply as PROXY_API_KEYS if that var is not already set.
    pub litellm_master_key: Option<String>,
    /// Tool-related config from a simple YAML config file. None when the config
    /// was loaded from env vars, TOML, or LiteLLM format (which has no tool sections).
    pub tool_config: Option<crate::config::simple::ToolStartupConfig>,
}

impl MultiConfig {
    /// Load configuration.
    ///
    /// Detection order:
    /// 1. `PROXY_CONFIG` with `.yaml`/`.yml` extension:
    ///    - If root `models:` key is present: simple native format (`simple::parse_simple_yaml`)
    ///    - Otherwise (`model_list:` key): LiteLLM-compatible format (`litellm::parse_litellm_yaml`)
    /// 2. `PROXY_CONFIG` with any other extension: parse as TOML
    /// 3. No `PROXY_CONFIG`: env-var-based single-backend config
    ///
    /// The model router is set for both YAML config formats (simple and LiteLLM).
    /// `litellm_master_key` is returned (not applied) so the caller can
    /// consolidate all `set_var` calls into a single pre-runtime block.
    pub fn load() -> LoadResult {
        if let Ok(path) = std::env::var("PROXY_CONFIG") {
            if path.ends_with(".yaml") || path.ends_with(".yml") {
                let yaml = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read config '{path}': {e}"));

                // Detect format: "models:" key = simple native format, "model_list:" = LiteLLM.
                let probe: serde_yaml::Value = serde_yaml::from_str(&yaml)
                    .unwrap_or_else(|e| panic!("invalid YAML in '{path}': {e}"));

                if probe.get("models").is_some() {
                    // Simple native format.
                    let parsed = crate::config::simple::parse_simple_yaml(&yaml);
                    return LoadResult {
                        multi_config: parsed.multi_config,
                        model_router: Some(Arc::new(std::sync::RwLock::new(parsed.router))),
                        litellm_master_key: None,
                        tool_config: Some(parsed.tool_config),
                    };
                }

                // LiteLLM format requires model_list: key.
                if probe.get("model_list").is_none() {
                    panic!(
                        "config file '{path}' must contain either a top-level 'models:' key \
                         (simple format) or 'model_list:' key (LiteLLM format)"
                    );
                }

                // LiteLLM format (model_list: + litellm_params:).
                let parsed = crate::config::litellm::parse_litellm_yaml(&yaml);

                // Wire up webhook callbacks and named integrations from litellm_settings.callbacks.
                let mut named = vec![];
                if parsed.langfuse_requested {
                    match crate::integrations::LangfuseClient::from_env() {
                        Some(lf) => {
                            tracing::info!("langfuse integration enabled");
                            named.push(crate::integrations::NamedIntegration::Langfuse(lf));
                        }
                        None => tracing::warn!(
                            "langfuse in litellm_settings.callbacks but LANGFUSE_PUBLIC_KEY/SECRET not set"
                        ),
                    }
                }
                if let Some(cb) =
                    crate::callbacks::CallbackConfig::with_named(parsed.callback_urls, named)
                {
                    crate::server::routes::set_callbacks(cb);
                    tracing::info!("callbacks configured from litellm_settings");
                }

                let mut mc = parsed.multi_config;
                // PROXY_CONFIG is set (we're in this branch): auto-enable warnings.
                mc.expose_degradation_warnings = true;
                return LoadResult {
                    multi_config: mc,
                    model_router: Some(Arc::new(std::sync::RwLock::new(parsed.router))),
                    litellm_master_key: parsed.master_key,
                    // LiteLLM format has no tool sections. This is intentional, not a
                    // TODO: tool_execution/guardrails are documented as simple-YAML-only
                    // (see docs/CONFIG.md and docs/codedocs/configuration-and-modes.md);
                    // LiteLLM users must use FORGE_TOOL_CALL_POLICY instead.
                    tool_config: None,
                };
            }
            LoadResult {
                multi_config: Self::from_toml_file(&path),
                model_router: None,
                litellm_master_key: None,
                tool_config: None,
            }
        } else {
            LoadResult {
                multi_config: Self::from_legacy_env(),
                model_router: None,
                litellm_master_key: None,
                tool_config: None,
            }
        }
    }

    /// Wrap a single-backend Config into a MultiConfig.
    /// Used by the legacy `app(config)` path and by `from_legacy_env`.
    pub fn from_single_config(config: &Config) -> Self {
        Self::wrap_config(config)
    }

    /// Wrap the existing single-backend Config into a MultiConfig.
    fn from_legacy_env() -> Self {
        let config = Config::from_env();
        Self::wrap_config(&config)
    }

    fn wrap_config(config: &Config) -> Self {
        let name = match config.backend {
            BackendKind::OpenAI => "openai",
            BackendKind::AzureOpenAI => "azure",
            BackendKind::Vertex => "vertex",
            BackendKind::Gemini => "gemini",
            BackendKind::Anthropic => "anthropic",
            BackendKind::Bedrock => "bedrock",
        };

        let omit_stream_options = std::env::var("OMIT_STREAM_OPTIONS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let stream_timeout_secs = std::env::var("REQUEST_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900u64);

        // For Bedrock, read AWS credentials from env vars.
        let bedrock_credentials = if config.backend == BackendKind::Bedrock {
            Some(bedrock_credentials_from_env())
        } else {
            None
        };

        let bc = BackendConfig {
            kind: config.backend.clone(),
            provider_id: config.provider_id.clone(),
            api_key: config.openai_api_key.clone(),
            base_url: config.openai_base_url.clone(),
            api_format: config.openai_api_format.clone(),
            model_mapping: config.model_mapping.clone(),
            tls: config.tls.clone(),
            backend_auth: config.backend_auth.clone(),
            log_bodies: config.log_bodies,
            omit_stream_options,
            stream_timeout_secs,
            bedrock_credentials,
        };

        let mut backends = IndexMap::new();
        backends.insert(name.to_string(), bc);

        Self {
            listen_port: config.listen_port,
            log_bodies: config.log_bodies,
            redact_secrets: config.redact_secrets,
            anthropic_thinking_repair: config.anthropic_thinking_repair,
            forward_client_auth: crate::config::env_bool_flag("ANTHROPIC_FORWARD_CLIENT_AUTH"),
            default_backend: name.to_string(),
            backends,
            expose_degradation_warnings: config.expose_degradation_warnings,
        }
    }
}
