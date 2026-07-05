use super::helpers::{sanitize_api_key, strip_v1_suffix};
use super::tls::TlsConfig;
use super::types::{BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat, GEMINI_OPENAI_PATH};
use super::url_validation::validate_base_url;

/// Proxy configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub backend: BackendKind,
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub listen_port: u16,
    pub model_mapping: ModelMapping,
    pub tls: TlsConfig,
    pub backend_auth: BackendAuth,
    /// Enable request/response body logging at debug level.
    pub log_bodies: bool,
    /// Redact detected secrets from upstream JSON/text request payloads.
    pub redact_secrets: bool,
    /// Enable Anthropic thinking-block record-and-restore repair (BACKEND=anthropic passthrough only).
    pub anthropic_thinking_repair: bool,
    /// Enable text-to-image context compression (pxpipe; BACKEND=anthropic passthrough only).
    pub pxpipe_compress: bool,
    /// Expose `x-anyllm-degradation` response header when features are silently dropped.
    /// Defaults to false (simple mode). Enable with ANYLLM_DEGRADATION_WARNINGS=true
    /// or automatically when PROXY_CONFIG is set.
    pub expose_degradation_warnings: bool,
    /// Which OpenAI API format to use (only relevant when BACKEND=openai).
    pub openai_api_format: OpenAIApiFormat,
    /// Provider ID when backend has provider-specific policy.
    pub provider_id: Option<String>,
}

/// Validate that a GCP identifier (project ID, region) contains only safe characters.
/// Prevents URL injection when these values are interpolated into Vertex AI endpoint URLs.
pub(crate) fn validate_gcp_identifier(name: &str, value: &str) {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        panic!(
            "{name} contains invalid characters: only alphanumeric, '-', '_', '.' are allowed, got: {value}"
        );
    }
}

impl Config {
    /// Build configuration from environment variables. Panics on invalid values
    /// (unknown backend, bad GCP identifiers) to fail fast at startup.
    pub fn from_env() -> Self {
        let backend_str = std::env::var("BACKEND").unwrap_or_else(|_| "openai".into());
        // For stub OpenAI-compatible providers (e.g. BACKEND=groq), record the provider def so
        // the OpenAI config branch can pick up the correct API key env var and default base URL.
        let mut stub_provider: Option<&'static anyllm_providers::ProviderDef> = None;
        let backend = match backend_str.to_ascii_lowercase().as_str() {
            "openai" => BackendKind::OpenAI,
            "azure" => BackendKind::AzureOpenAI,
            "vertex" | "vertex_ai" => BackendKind::Vertex,
            "gemini" => BackendKind::Gemini,
            "anthropic" => BackendKind::Anthropic,
            "bedrock" => BackendKind::Bedrock,
            other => {
                match anyllm_providers::resolve_backend(other) {
                    Some(("openai", _)) => {
                        stub_provider = anyllm_providers::get_provider(other);
                        BackendKind::OpenAI
                    }
                    Some((kind, _)) => panic!(
                        "BACKEND={other} is a known provider but requires direct configuration \
                         (protocol: {kind}); use PROXY_CONFIG with a TOML or LiteLLM YAML file instead"
                    ),
                    None => {
                        let known: Vec<&str> = anyllm_providers::all_providers()
                            .map(|p| p.id)
                            .collect();
                        panic!(
                            "unknown BACKEND value '{other}'. \
                             Known values: openai, azure, vertex, gemini, anthropic, bedrock, \
                             and provider ids: {}",
                            known.join(", ")
                        )
                    }
                }
            }
        };

        let listen_port: u16 = match std::env::var("LISTEN_PORT") {
            Ok(val) => val
                .parse::<u16>()
                .unwrap_or_else(|_| panic!("LISTEN_PORT must be a number in 1-65535, got '{val}'")),
            Err(_) => 3000,
        };
        if listen_port == 0 {
            panic!("LISTEN_PORT cannot be 0");
        }
        if listen_port < 1024 {
            eprintln!(
                "warning: LISTEN_PORT {listen_port} is in the privileged range (< 1024); \
                 binding may fail without elevated privileges"
            );
        }
        let tls = TlsConfig::from_env();
        let log_bodies = std::env::var("LOG_BODIES")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let redact_secrets = std::env::var("REDACT_SECRETS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let anthropic_thinking_repair = std::env::var("ANTHROPIC_THINKING_REPAIR")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let pxpipe_compress = std::env::var("PXPIPE_COMPRESS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let expose_degradation_warnings = std::env::var("ANYLLM_DEGRADATION_WARNINGS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        // Config file presence implies advanced mode — enable degradation warnings.
        let expose_degradation_warnings =
            expose_degradation_warnings || std::env::var("PROXY_CONFIG").is_ok();

        match backend {
            BackendKind::OpenAI => {
                let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| {
                    if let Some(provider) = stub_provider {
                        if provider.default_base_url.is_empty() {
                            panic!(
                                "BACKEND={} requires OPENAI_BASE_URL because this provider has no safe global API base URL",
                                provider.id
                            );
                        }
                        provider.default_base_url.to_string()
                    } else {
                        "https://api.openai.com".to_string()
                    }
                });
                let base_url = strip_v1_suffix(&base_url).to_string();
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("OPENAI_BASE_URL rejected: {e}");
                }
                // For stub providers, fall back to their env var (e.g. GROQ_API_KEY) when
                // OPENAI_API_KEY is not set.
                let api_key =
                    sanitize_api_key(&std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
                        stub_provider
                            .and_then(|p| p.env_vars.iter().find_map(|v| std::env::var(v).ok()))
                            .unwrap_or_default()
                    }));
                let backend_auth = BackendAuth::BearerToken(api_key.clone());
                let openai_api_format = match std::env::var("OPENAI_API_FORMAT")
                    .unwrap_or_else(|_| "chat".into())
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "chat" => OpenAIApiFormat::Chat,
                    "responses" => OpenAIApiFormat::Responses,
                    other => panic!(
                        "unknown OPENAI_API_FORMAT value '{other}', expected 'chat' or 'responses'"
                    ),
                };
                Self {
                    backend,
                    openai_api_key: api_key,
                    openai_base_url: base_url,
                    listen_port,
                    model_mapping: ModelMapping::from_env_with_defaults("gpt-4o", "gpt-4o-mini"),
                    tls,
                    backend_auth,
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format,
                    provider_id: stub_provider.map(|p| p.id.to_string()),
                }
            }
            BackendKind::AzureOpenAI => {
                let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT").unwrap_or_else(|_| {
                    panic!("AZURE_OPENAI_ENDPOINT is required when BACKEND=azure")
                });
                let deployment = std::env::var("AZURE_OPENAI_DEPLOYMENT").unwrap_or_else(|_| {
                    panic!("AZURE_OPENAI_DEPLOYMENT is required when BACKEND=azure")
                });
                let api_key =
                    sanitize_api_key(&std::env::var("AZURE_OPENAI_API_KEY").unwrap_or_else(|_| {
                        panic!("AZURE_OPENAI_API_KEY is required when BACKEND=azure")
                    }));
                let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
                    .unwrap_or_else(|_| "2024-10-21".to_string());

                // Pre-construct the full URL; no suffix is appended by OpenAIClient.
                let base_url = format!(
                    "{}/openai/deployments/{}/chat/completions?api-version={}",
                    endpoint.trim_end_matches('/'),
                    deployment,
                    api_version
                );
                // Validate the endpoint (not the full URL, which has query params)
                if let Err(e) = validate_base_url(endpoint.trim_end_matches('/')) {
                    panic!("AZURE_OPENAI_ENDPOINT rejected: {e}");
                }

                Self {
                    backend,
                    openai_api_key: String::new(),
                    openai_base_url: base_url,
                    listen_port,
                    model_mapping: ModelMapping::from_env_with_defaults("gpt-4o", "gpt-4o-mini"),
                    tls,
                    backend_auth: BackendAuth::AzureApiKey(api_key),
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format: OpenAIApiFormat::Chat,
                    provider_id: None,
                }
            }
            BackendKind::Vertex => {
                let project = std::env::var("VERTEX_PROJECT")
                    .unwrap_or_else(|_| panic!("VERTEX_PROJECT is required when BACKEND=vertex"));
                let region = std::env::var("VERTEX_REGION")
                    .unwrap_or_else(|_| panic!("VERTEX_REGION is required when BACKEND=vertex"));
                validate_gcp_identifier("VERTEX_PROJECT", &project);
                validate_gcp_identifier("VERTEX_REGION", &region);

                let backend_auth = if let Ok(api_key) = std::env::var("VERTEX_API_KEY") {
                    BackendAuth::GoogleApiKey(sanitize_api_key(&api_key))
                } else if let Ok(token) = std::env::var("GOOGLE_ACCESS_TOKEN") {
                    BackendAuth::BearerToken(sanitize_api_key(&token))
                } else {
                    panic!("VERTEX_API_KEY or GOOGLE_ACCESS_TOKEN is required when BACKEND=vertex");
                };

                let base_url = format!(
                    "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/endpoints/openapi"
                );
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("Vertex base URL rejected: {e}");
                }

                Self {
                    backend,
                    openai_api_key: String::new(),
                    openai_base_url: base_url,
                    listen_port,
                    model_mapping: ModelMapping::from_env_with_defaults(
                        "gemini-2.5-pro",
                        "gemini-2.5-flash",
                    ),
                    tls,
                    backend_auth,
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format: OpenAIApiFormat::Chat,
                    provider_id: None,
                }
            }
            BackendKind::Gemini => {
                let api_key =
                    sanitize_api_key(&std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| {
                        panic!("GEMINI_API_KEY is required when BACKEND=gemini")
                    }));

                let base_url = std::env::var("GEMINI_BASE_URL").unwrap_or_else(|_| {
                    "https://generativelanguage.googleapis.com/v1beta".to_string()
                });
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("Gemini base URL rejected: {e}");
                }

                let backend_auth = BackendAuth::GoogleApiKey(api_key);

                Self {
                    backend,
                    openai_api_key: String::new(),
                    openai_base_url: format!("{base_url}{GEMINI_OPENAI_PATH}"),
                    listen_port,
                    model_mapping: ModelMapping::from_env_with_defaults(
                        "gemini-2.5-pro",
                        "gemini-2.5-flash",
                    ),
                    tls,
                    backend_auth,
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format: OpenAIApiFormat::Chat,
                    provider_id: None,
                }
            }
            BackendKind::Anthropic => {
                let backend_auth = if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
                    BackendAuth::anthropic_from_api_key_like(sanitize_api_key(&api_key))
                } else if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
                    BackendAuth::AnthropicAuthToken(sanitize_api_key(&token))
                } else {
                    panic!(
                        "ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN is required when BACKEND=anthropic"
                    )
                };

                let base_url = std::env::var("ANTHROPIC_BASE_URL")
                    .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("ANTHROPIC_BASE_URL rejected: {e}");
                }

                Self {
                    backend,
                    openai_api_key: String::new(),
                    openai_base_url: base_url,
                    listen_port,
                    model_mapping: ModelMapping {
                        big_model: String::new(),
                        small_model: String::new(),
                    },
                    tls,
                    backend_auth,
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format: OpenAIApiFormat::Chat,
                    provider_id: None,
                }
            }
            BackendKind::Bedrock => {
                let region = std::env::var("AWS_REGION")
                    .unwrap_or_else(|_| panic!("AWS_REGION is required when BACKEND=bedrock"));
                validate_gcp_identifier("AWS_REGION", &region); // reuse safe-char validation

                // Validate credentials are present at startup; the actual values
                // are read again when constructing BedrockClient.
                let _access_key_id = std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| {
                    panic!("AWS_ACCESS_KEY_ID is required when BACKEND=bedrock")
                });
                let _secret_access_key =
                    std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| {
                        panic!("AWS_SECRET_ACCESS_KEY is required when BACKEND=bedrock")
                    });
                let _session_token = std::env::var("AWS_SESSION_TOKEN").ok();

                Self {
                    backend,
                    openai_api_key: String::new(),
                    // Store region in openai_base_url for wrap_config
                    openai_base_url: region.clone(),
                    listen_port,
                    model_mapping: ModelMapping::from_env_with_defaults(
                        "anthropic.claude-sonnet-4-20250514-v1:0",
                        "anthropic.claude-haiku-4-5-20251001-v1:0",
                    ),
                    tls,
                    backend_auth: BackendAuth::BearerToken(String::new()),
                    log_bodies,
                    redact_secrets,
                    anthropic_thinking_repair,
                    pxpipe_compress,
                    expose_degradation_warnings,
                    openai_api_format: OpenAIApiFormat::Chat,
                    provider_id: None,
                }
            }
        }
    }
}

/// Read AWS credentials from environment variables for the Bedrock backend.
pub(crate) fn bedrock_credentials_from_env() -> aws_credential_types::Credentials {
    let access_key_id = std::env::var("AWS_ACCESS_KEY_ID")
        .unwrap_or_else(|_| panic!("AWS_ACCESS_KEY_ID is required for bedrock"));
    let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
        .unwrap_or_else(|_| panic!("AWS_SECRET_ACCESS_KEY is required for bedrock"));
    let session_token = std::env::var("AWS_SESSION_TOKEN").ok();
    aws_credential_types::Credentials::new(
        access_key_id,
        secret_access_key,
        session_token,
        None,
        "env",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_anthropic_env() {
        unsafe {
            std::env::remove_var("BACKEND");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
            std::env::remove_var("ANTHROPIC_BASE_URL");
            std::env::remove_var("PROXY_CONFIG");
        }
    }

    #[test]
    fn anthropic_auth_token_env_uses_bearer_auth() {
        let _lock = crate::config::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_anthropic_env();
        unsafe {
            std::env::set_var("BACKEND", "anthropic");
            std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-ant-oat-env");
        }

        let config = Config::from_env();
        assert_eq!(
            config.backend_auth,
            BackendAuth::AnthropicAuthToken("sk-ant-oat-env".to_string())
        );

        clear_anthropic_env();
    }

    #[test]
    fn anthropic_api_key_precedes_auth_token_env() {
        let _lock = crate::config::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_anthropic_env();
        unsafe {
            std::env::set_var("BACKEND", "anthropic");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api-env");
            std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-ant-oat-env");
        }

        let config = Config::from_env();
        assert_eq!(
            config.backend_auth,
            BackendAuth::AnthropicApiKey("sk-ant-api-env".to_string())
        );

        clear_anthropic_env();
    }

    #[test]
    fn backend_vertex_ai_alias_uses_vertex_config() {
        let _lock = crate::config::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_anthropic_env();
        unsafe {
            std::env::set_var("BACKEND", "vertex_ai");
            std::env::set_var("VERTEX_PROJECT", "project-123");
            std::env::set_var("VERTEX_REGION", "us-central1");
            std::env::set_var("VERTEX_API_KEY", "AIzaSy-test");
            std::env::remove_var("GOOGLE_ACCESS_TOKEN");
        }

        let config = Config::from_env();
        assert_eq!(config.backend, BackendKind::Vertex);
        assert_eq!(
            config.openai_base_url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/project-123/locations/us-central1/endpoints/openapi"
        );

        unsafe {
            std::env::remove_var("BACKEND");
            std::env::remove_var("VERTEX_PROJECT");
            std::env::remove_var("VERTEX_REGION");
            std::env::remove_var("VERTEX_API_KEY");
        }
    }
}
