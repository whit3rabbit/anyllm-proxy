use super::{BackendConfig, MultiConfig};
use crate::config::helpers::{resolve_env_value, sanitize_api_key};
use crate::config::single::validate_gcp_identifier;
use crate::config::tls::TlsConfig;
use crate::config::types::{
    BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat, GEMINI_OPENAI_PATH,
};
use crate::config::url_validation::validate_base_url;
use indexmap::IndexMap;
use serde::Deserialize;

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
    provider_id: Option<String>,
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

impl MultiConfig {
    /// Parse a TOML config file into MultiConfig.
    pub(super) fn from_toml_file(path: &str) -> Self {
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
        let anthropic_thinking_repair = std::env::var("ANTHROPIC_THINKING_REPAIR")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let pxpipe_compress = std::env::var("PXPIPE_COMPRESS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
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
            anthropic_thinking_repair,
            pxpipe_compress,
            forward_client_auth: crate::config::env_bool_flag("ANTHROPIC_FORWARD_CLIENT_AUTH"),
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
                let auth = BackendAuth::anthropic_from_api_key_like(api_key.clone());
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
            provider_id: tb.provider_id.clone(),
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
