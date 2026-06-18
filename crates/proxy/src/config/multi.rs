use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::Arc;

use super::helpers::{resolve_env_value, sanitize_api_key};
use super::single::{bedrock_credentials_from_env, validate_gcp_identifier, Config};
use super::tls::TlsConfig;
use super::types::{BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat, GEMINI_OPENAI_PATH};
use super::url_validation::validate_base_url;

/// Per-backend configuration. Each entry in `[backends.*]` deserializes into this.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Which provider type this backend uses (OpenAI, Vertex, Gemini, Anthropic).
    pub kind: BackendKind,
    /// API key for authentication. Resolved from env vars via `env:VAR_NAME` syntax.
    pub api_key: String,
    /// Base URL of the backend API (e.g., `https://api.openai.com`).
    pub base_url: String,
    /// Which OpenAI API format to use (Chat Completions or Responses).
    pub api_format: OpenAIApiFormat,
    /// Anthropic-to-backend model name mapping.
    pub model_mapping: ModelMapping,
    /// Optional mTLS and custom CA configuration.
    pub tls: TlsConfig,
    /// How to authenticate to this backend (Bearer token or Google API key).
    pub backend_auth: BackendAuth,
    /// Whether to log request/response bodies at debug level.
    pub log_bodies: bool,
    /// Strip `stream_options` from streaming requests. Needed for local LLMs
    /// (older Ollama, text-generation-webui, LM Studio) that reject unknown
    /// fields with HTTP 400.
    pub omit_stream_options: bool,
    /// Wall-clock cap for streaming responses in seconds. 0 = disabled.
    pub stream_timeout_secs: u64,
    /// AWS credentials for Bedrock backend. None for all other backends.
    pub bedrock_credentials: Option<aws_credential_types::Credentials>,
}

/// Top-level multi-backend configuration loaded from TOML.
/// Enables routing requests to different backends by route prefix.
#[derive(Debug, Clone)]
pub struct MultiConfig {
    /// Port the proxy listens on (default: 3000).
    pub listen_port: u16,
    /// Whether to log request/response bodies at debug level (global default).
    pub log_bodies: bool,
    /// Redact detected secrets from upstream JSON/text request payloads.
    pub redact_secrets: bool,
    /// Backend name used when no route prefix matches.
    pub default_backend: String,
    /// Ordered map: key = route prefix (e.g. "openai"), value = backend config.
    pub backends: IndexMap<String, BackendConfig>,
    /// See Config::expose_degradation_warnings.
    pub expose_degradation_warnings: bool,
}

// -- TOML deserialization structs (separate from runtime types) --

#[derive(Deserialize)]
struct TomlConfig {
    listen_port: Option<u16>,
    log_bodies: Option<bool>,
    redact_secrets: Option<bool>,
    default_backend: Option<String>,
    #[serde(default)]
    expose_degradation_warnings: bool,
    #[serde(default)]
    backends: IndexMap<String, TomlBackendConfig>,
}

#[derive(Deserialize)]
struct TomlBackendConfig {
    kind: String,
    api_key: Option<String>,
    base_url: Option<String>,
    api_format: Option<String>,
    big_model: Option<String>,
    small_model: Option<String>,
    // Vertex-specific
    project: Option<String>,
    region: Option<String>,
    // Azure-specific
    endpoint: Option<String>,
    deployment: Option<String>,
    api_version: Option<String>,
    // Optional env var name for Google access token (Vertex)
    access_token: Option<String>,
    // Strip stream_options from streaming requests (local LLM compat)
    omit_stream_options: Option<bool>,
    // Wall-clock cap for streaming responses in seconds (0 = disabled)
    stream_timeout_secs: Option<u64>,
    // Bedrock-specific: AWS credentials (support env: prefix for env var resolution)
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
    aws_session_token: Option<String>,
}

/// Result of `MultiConfig::load()`.
pub struct LoadResult {
    pub multi_config: MultiConfig,
    pub model_router: Option<Arc<std::sync::RwLock<super::model_router::ModelRouter>>>,
    /// Resolved master_key from LiteLLM general_settings, if present.
    /// Caller should apply as PROXY_API_KEYS if that var is not already set.
    pub litellm_master_key: Option<String>,
    /// Tool-related config from a simple YAML config file. None when the config
    /// was loaded from env vars, TOML, or LiteLLM format (which has no tool sections).
    pub tool_config: Option<super::simple::ToolStartupConfig>,
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
                    let parsed = super::simple::parse_simple_yaml(&yaml);
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
                let parsed = super::litellm::parse_litellm_yaml(&yaml);

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
                    tool_config: None, // LiteLLM format has no tool sections
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
            default_backend: name.to_string(),
            backends,
            expose_degradation_warnings: config.expose_degradation_warnings,
        }
    }

    /// Parse a TOML config file into MultiConfig.
    fn from_toml_file(path: &str) -> Self {
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read config file '{path}': {e}"));
        Self::from_toml_str(&contents)
    }

    /// Parse TOML string into MultiConfig. Separated from file I/O for testing.
    pub fn from_toml_str(toml_str: &str) -> Self {
        let raw: TomlConfig =
            toml::from_str(toml_str).unwrap_or_else(|e| panic!("invalid TOML config: {e}"));

        if raw.backends.is_empty() {
            panic!("config must define at least one backend in [backends.*]");
        }

        let listen_port = raw.listen_port.unwrap_or(3000);
        let log_bodies = raw.log_bodies.unwrap_or(false);
        let redact_secrets = raw.redact_secrets.unwrap_or_else(|| {
            std::env::var("REDACT_SECRETS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });
        let default_backend = raw
            .default_backend
            .unwrap_or_else(|| raw.backends.keys().next().unwrap().clone());

        if !raw.backends.contains_key(&default_backend) {
            panic!(
                "default_backend '{default_backend}' not found in configured backends: {:?}",
                raw.backends.keys().collect::<Vec<_>>()
            );
        }

        let tls = TlsConfig::from_env();
        let mut backends = IndexMap::new();

        for (name, tb) in &raw.backends {
            let bc = Self::build_backend_config(name, tb, &tls, log_bodies);
            backends.insert(name.clone(), bc);
        }

        // OR-in: TOML field || env var || PROXY_CONFIG presence (auto-enable).
        let expose_degradation_warnings = raw.expose_degradation_warnings
            || std::env::var("ANYLLM_DEGRADATION_WARNINGS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
            || std::env::var("PROXY_CONFIG").is_ok();

        Self {
            listen_port,
            log_bodies,
            redact_secrets,
            default_backend,
            backends,
            expose_degradation_warnings,
        }
    }

    fn build_backend_config(
        name: &str,
        tb: &TomlBackendConfig,
        tls: &TlsConfig,
        log_bodies: bool,
    ) -> BackendConfig {
        let kind = match tb.kind.to_ascii_lowercase().as_str() {
            "openai" => BackendKind::OpenAI,
            "azure" => BackendKind::AzureOpenAI,
            "vertex" => BackendKind::Vertex,
            "gemini" => BackendKind::Gemini,
            "anthropic" => BackendKind::Anthropic,
            "bedrock" => BackendKind::Bedrock,
            other => panic!("unknown backend kind '{other}' for backend '{name}'"),
        };

        let api_key = sanitize_api_key(
            &tb.api_key
                .as_deref()
                .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
                .unwrap_or_default(),
        );

        let (base_url, backend_auth, model_mapping, api_format) = match &kind {
            BackendKind::OpenAI => {
                let base_url = tb
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com".to_string());
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("backend '{name}' base_url rejected: {e}");
                }
                let auth = BackendAuth::BearerToken(api_key.clone());
                let fmt = match tb
                    .api_format
                    .as_deref()
                    .unwrap_or("chat")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "chat" => OpenAIApiFormat::Chat,
                    "responses" => OpenAIApiFormat::Responses,
                    other => panic!("unknown api_format '{other}' for backend '{name}'"),
                };
                let mm = ModelMapping {
                    big_model: tb.big_model.clone().unwrap_or_else(|| "gpt-4o".to_string()),
                    small_model: tb
                        .small_model
                        .clone()
                        .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                };
                (base_url, auth, mm, fmt)
            }
            BackendKind::AzureOpenAI => {
                if api_key.is_empty() {
                    panic!("backend '{name}': api_key is required for azure");
                }
                let endpoint = tb.endpoint.as_deref().unwrap_or_else(|| {
                    panic!("backend '{name}': 'endpoint' is required for azure")
                });
                let deployment = tb.deployment.as_deref().unwrap_or_else(|| {
                    panic!("backend '{name}': 'deployment' is required for azure")
                });
                let api_version = tb.api_version.as_deref().unwrap_or("2024-10-21");

                if let Err(e) = validate_base_url(endpoint.trim_end_matches('/')) {
                    panic!("backend '{name}' endpoint rejected: {e}");
                }

                let base_url = format!(
                    "{}/openai/deployments/{}/chat/completions?api-version={}",
                    endpoint.trim_end_matches('/'),
                    deployment,
                    api_version
                );
                let auth = BackendAuth::AzureApiKey(api_key.clone());
                let mm = ModelMapping {
                    big_model: tb.big_model.clone().unwrap_or_else(|| "gpt-4o".to_string()),
                    small_model: tb
                        .small_model
                        .clone()
                        .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                };
                (base_url, auth, mm, OpenAIApiFormat::Chat)
            }
            BackendKind::Vertex => {
                let project = tb.project.as_deref().unwrap_or_else(|| {
                    panic!("backend '{name}': 'project' is required for vertex")
                });
                let region = tb
                    .region
                    .as_deref()
                    .unwrap_or_else(|| panic!("backend '{name}': 'region' is required for vertex"));
                validate_gcp_identifier("project", project);
                validate_gcp_identifier("region", region);

                let base_url = tb.base_url.clone().unwrap_or_else(|| {
                    format!(
                        "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/endpoints/openapi"
                    )
                });
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("backend '{name}' base_url rejected: {e}");
                }

                let auth = if !api_key.is_empty() {
                    BackendAuth::GoogleApiKey(api_key.clone())
                } else if let Some(token_ref) = &tb.access_token {
                    let token = sanitize_api_key(
                        &resolve_env_value(token_ref)
                            .unwrap_or_else(|e| panic!("backend '{name}': {e}")),
                    );
                    BackendAuth::BearerToken(token)
                } else {
                    panic!("backend '{name}': api_key or access_token is required for vertex");
                };

                let mm = ModelMapping {
                    big_model: tb
                        .big_model
                        .clone()
                        .unwrap_or_else(|| "gemini-2.5-pro".to_string()),
                    small_model: tb
                        .small_model
                        .clone()
                        .unwrap_or_else(|| "gemini-2.5-flash".to_string()),
                };
                (base_url, auth, mm, OpenAIApiFormat::Chat)
            }
            BackendKind::Gemini => {
                if api_key.is_empty() {
                    panic!("backend '{name}': api_key is required for gemini");
                }
                let base_url = tb.base_url.clone().unwrap_or_else(|| {
                    "https://generativelanguage.googleapis.com/v1beta".to_string()
                });
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("backend '{name}' base_url rejected: {e}");
                }
                let auth = BackendAuth::GoogleApiKey(api_key.clone());
                let mm = ModelMapping {
                    big_model: tb
                        .big_model
                        .clone()
                        .unwrap_or_else(|| "gemini-2.5-pro".to_string()),
                    small_model: tb
                        .small_model
                        .clone()
                        .unwrap_or_else(|| "gemini-2.5-flash".to_string()),
                };

                (
                    format!("{base_url}{GEMINI_OPENAI_PATH}"),
                    auth,
                    mm,
                    OpenAIApiFormat::Chat,
                )
            }
            BackendKind::Anthropic => {
                if api_key.is_empty() {
                    panic!("backend '{name}': api_key is required for anthropic");
                }
                let base_url = tb
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.anthropic.com".to_string());
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("backend '{name}' base_url rejected: {e}");
                }
                // Anthropic uses x-api-key header, stored as BearerToken for simplicity
                // (the AnthropicClient will apply it correctly)
                let auth = BackendAuth::BearerToken(api_key.clone());
                // No model mapping needed for passthrough
                let mm = ModelMapping {
                    big_model: String::new(),
                    small_model: String::new(),
                };
                (base_url, auth, mm, OpenAIApiFormat::Chat)
            }
            BackendKind::Bedrock => {
                let region = tb.region.as_deref().unwrap_or_else(|| {
                    panic!("backend '{name}': 'region' is required for bedrock")
                });
                validate_gcp_identifier("region", region);

                // For Bedrock, base_url stores the region (used by BedrockClient to build URLs)
                let auth = BackendAuth::BearerToken(String::new());
                let mm =
                    ModelMapping {
                        big_model: tb.big_model.clone().unwrap_or_else(|| {
                            "anthropic.claude-sonnet-4-20250514-v1:0".to_string()
                        }),
                        small_model: tb.small_model.clone().unwrap_or_else(|| {
                            "anthropic.claude-haiku-4-5-20251001-v1:0".to_string()
                        }),
                    };
                (region.to_string(), auth, mm, OpenAIApiFormat::Chat)
            }
        };

        // Build AWS credentials for Bedrock from TOML fields or env vars.
        let bedrock_credentials = if kind == BackendKind::Bedrock {
            let access_key_id = tb
                .aws_access_key_id
                .as_deref()
                .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
                .unwrap_or_else(|| {
                    std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| {
                        panic!("backend '{name}': aws_access_key_id or AWS_ACCESS_KEY_ID required")
                    })
                });
            let secret_access_key = tb
                .aws_secret_access_key
                .as_deref()
                .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
                .unwrap_or_else(|| {
                    std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| {
                        panic!(
                            "backend '{name}': aws_secret_access_key or AWS_SECRET_ACCESS_KEY required"
                        )
                    })
                });
            let session_token = tb
                .aws_session_token
                .as_deref()
                .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
                .or_else(|| std::env::var("AWS_SESSION_TOKEN").ok());
            Some(aws_credential_types::Credentials::new(
                access_key_id,
                secret_access_key,
                session_token,
                None,
                "toml-config",
            ))
        } else {
            None
        };

        BackendConfig {
            kind,
            api_key,
            base_url,
            api_format,
            model_mapping,
            tls: tls.clone(),
            backend_auth,
            log_bodies,
            omit_stream_options: tb.omit_stream_options.unwrap_or(false),
            stream_timeout_secs: tb.stream_timeout_secs.unwrap_or_else(|| {
                std::env::var("REQUEST_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(900u64)
            }),
            bedrock_credentials,
        }
    }
}

#[cfg(test)]
mod tests;
