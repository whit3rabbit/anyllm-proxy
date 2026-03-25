mod tls;
mod url_validation;

pub use tls::TlsConfig;
pub use url_validation::{is_private_ip, validate_base_url};

use indexmap::IndexMap;
use serde::Deserialize;
use std::fmt;

/// Path suffix appended to Gemini base URL to reach its OpenAI-compatible endpoint.
const GEMINI_OPENAI_PATH: &str = "/openai";

/// Which upstream backend the proxy targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    OpenAI,
    Vertex,
    Gemini,
    Anthropic,
}

/// Which OpenAI API format to use (only relevant when BACKEND=openai).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIApiFormat {
    /// Chat Completions API (default)
    Chat,
    /// Responses API
    Responses,
}

/// How the proxy authenticates to the upstream backend.
#[derive(Clone)]
pub enum BackendAuth {
    /// `Authorization: Bearer {token}` (OpenAI, Vertex OAuth)
    BearerToken(String),
    /// `x-goog-api-key: {key}` (Vertex API key)
    GoogleApiKey(String),
}

impl fmt::Debug for BackendAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BearerToken(_) => write!(f, "BearerToken([REDACTED])"),
            Self::GoogleApiKey(_) => write!(f, "GoogleApiKey([REDACTED])"),
        }
    }
}

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
    /// Which OpenAI API format to use (only relevant when BACKEND=openai).
    pub openai_api_format: OpenAIApiFormat,
}

/// Validate that a GCP identifier (project ID, region) contains only safe characters.
/// Prevents URL injection when these values are interpolated into Vertex AI endpoint URLs.
fn validate_gcp_identifier(name: &str, value: &str) {
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
        let backend = match backend_str.to_ascii_lowercase().as_str() {
            "openai" => BackendKind::OpenAI,
            "vertex" => BackendKind::Vertex,
            "gemini" => BackendKind::Gemini,
            "anthropic" => BackendKind::Anthropic,
            other => {
                panic!("unknown BACKEND value '{other}', expected 'openai', 'vertex', 'gemini', or 'anthropic'")
            }
        };

        let listen_port = std::env::var("LISTEN_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let tls = TlsConfig::from_env();
        let log_bodies = std::env::var("LOG_BODIES")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        match backend {
            BackendKind::OpenAI => {
                let base_url = std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com".to_string());
                if let Err(e) = validate_base_url(&base_url) {
                    panic!("OPENAI_BASE_URL rejected: {e}");
                }
                let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
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
                    openai_api_format,
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
                    BackendAuth::GoogleApiKey(api_key)
                } else if let Ok(token) = std::env::var("GOOGLE_ACCESS_TOKEN") {
                    BackendAuth::BearerToken(token)
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
                    openai_api_format: OpenAIApiFormat::Chat,
                }
            }
            BackendKind::Gemini => {
                let api_key = std::env::var("GEMINI_API_KEY")
                    .unwrap_or_else(|_| panic!("GEMINI_API_KEY is required when BACKEND=gemini"));

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
                    openai_api_format: OpenAIApiFormat::Chat,
                }
            }
            BackendKind::Anthropic => {
                let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
                    panic!("ANTHROPIC_API_KEY is required when BACKEND=anthropic")
                });

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
                    backend_auth: BackendAuth::BearerToken(api_key),
                    log_bodies,
                    openai_api_format: OpenAIApiFormat::Chat,
                }
            }
        }
    }
}

/// Maps Anthropic model names to OpenAI model names.
/// Pattern: "haiku" -> small_model, "sonnet"/"opus" -> big_model.
/// Unrecognized models pass through with a warning.
#[derive(Debug, Clone)]
pub struct ModelMapping {
    pub big_model: String,
    pub small_model: String,
}

impl ModelMapping {
    /// Load model mapping from `BIG_MODEL` / `SMALL_MODEL` env vars with OpenAI defaults.
    pub fn from_env() -> Self {
        Self::from_env_with_defaults("gpt-4o", "gpt-4o-mini")
    }

    /// Load model mapping from env vars, falling back to the provided defaults.
    /// Each backend calls this with its own defaults (e.g., Gemini uses `gemini-2.5-pro`).
    pub fn from_env_with_defaults(big_default: &str, small_default: &str) -> Self {
        Self {
            big_model: std::env::var("BIG_MODEL").unwrap_or_else(|_| big_default.into()),
            small_model: std::env::var("SMALL_MODEL").unwrap_or_else(|_| small_default.into()),
        }
    }

    /// Map an Anthropic model name to the configured OpenAI model.
    pub fn map_model(&self, model: &str) -> String {
        // ASCII case-insensitive substring check avoids allocating a lowercase copy.
        let bytes = model.as_bytes();
        if contains_ignore_ascii_case(bytes, b"haiku") {
            self.small_model.clone()
        } else if contains_ignore_ascii_case(bytes, b"sonnet")
            || contains_ignore_ascii_case(bytes, b"opus")
        {
            self.big_model.clone()
        } else {
            tracing::warn!(model = %model, "unrecognized model name, passing through unchanged");
            model.to_string()
        }
    }
}

fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

// ---------------------------------------------------------------------------
// Multi-backend configuration
// ---------------------------------------------------------------------------

/// Resolve a config value that may reference an env var via `env:VAR_NAME` prefix.
/// This allows TOML config files to reference secrets from the environment
/// without hardcoding them, keeping credentials out of version control.
pub fn resolve_env_value(value: &str) -> Result<String, String> {
    if let Some(var_name) = value.strip_prefix("env:") {
        std::env::var(var_name)
            .map_err(|_| format!("env var '{var_name}' referenced in config is not set"))
    } else {
        Ok(value.to_string())
    }
}

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
}

/// Top-level multi-backend configuration loaded from TOML.
/// Enables routing requests to different backends by route prefix.
#[derive(Debug, Clone)]
pub struct MultiConfig {
    /// Port the proxy listens on (default: 3000).
    pub listen_port: u16,
    /// Whether to log request/response bodies at debug level (global default).
    pub log_bodies: bool,
    /// Backend name used when no route prefix matches.
    pub default_backend: String,
    /// Ordered map: key = route prefix (e.g. "openai"), value = backend config.
    pub backends: IndexMap<String, BackendConfig>,
}

// -- TOML deserialization structs (separate from runtime types) --

#[derive(Deserialize)]
struct TomlConfig {
    listen_port: Option<u16>,
    log_bodies: Option<bool>,
    default_backend: Option<String>,
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
    // Optional env var name for Google access token (Vertex)
    access_token: Option<String>,
    // Strip stream_options from streaming requests (local LLM compat)
    omit_stream_options: Option<bool>,
}

impl MultiConfig {
    /// Load configuration. If `PROXY_CONFIG` env var is set, parse the TOML file.
    /// Otherwise, fall back to existing env-var-based single-backend config.
    pub fn load() -> Self {
        if let Ok(path) = std::env::var("PROXY_CONFIG") {
            Self::from_toml_file(&path)
        } else {
            Self::from_legacy_env()
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
            BackendKind::Vertex => "vertex",
            BackendKind::Gemini => "gemini",
            BackendKind::Anthropic => "anthropic",
        };

        let omit_stream_options = std::env::var("OMIT_STREAM_OPTIONS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

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
        };

        let mut backends = IndexMap::new();
        backends.insert(name.to_string(), bc);

        Self {
            listen_port: config.listen_port,
            log_bodies: config.log_bodies,
            default_backend: name.to_string(),
            backends,
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

        Self {
            listen_port,
            log_bodies,
            default_backend,
            backends,
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
            "vertex" => BackendKind::Vertex,
            "gemini" => BackendKind::Gemini,
            "anthropic" => BackendKind::Anthropic,
            other => panic!("unknown backend kind '{other}' for backend '{name}'"),
        };

        let api_key = tb
            .api_key
            .as_deref()
            .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
            .unwrap_or_default();

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
                    let token = resolve_env_value(token_ref)
                        .unwrap_or_else(|e| panic!("backend '{name}': {e}"));
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_mapping_haiku() {
        let m = ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        };
        assert_eq!(m.map_model("claude-3-haiku-20240307"), "gpt-4o-mini");
        assert_eq!(m.map_model("claude-haiku-4-5-20251001"), "gpt-4o-mini");
    }

    #[test]
    fn model_mapping_sonnet() {
        let m = ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        };
        assert_eq!(m.map_model("claude-sonnet-4-6"), "gpt-4o");
        assert_eq!(m.map_model("claude-3-5-sonnet-20241022"), "gpt-4o");
    }

    #[test]
    fn model_mapping_opus() {
        let m = ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        };
        assert_eq!(m.map_model("claude-opus-4-6"), "gpt-4o");
    }

    #[test]
    fn model_mapping_passthrough() {
        let m = ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        };
        // Unrecognized models pass through unchanged
        assert_eq!(m.map_model("gpt-4o"), "gpt-4o");
        assert_eq!(m.map_model("custom-model"), "custom-model");
    }

    #[test]
    fn model_mapping_case_insensitive() {
        let m = ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        };
        assert_eq!(m.map_model("Claude-Sonnet-4-6"), "gpt-4o");
        assert_eq!(m.map_model("CLAUDE-HAIKU-4-5"), "gpt-4o-mini");
    }

    #[test]
    fn model_mapping_custom_values() {
        let m = ModelMapping {
            big_model: "o1-preview".into(),
            small_model: "o1-mini".into(),
        };
        assert_eq!(m.map_model("claude-sonnet-4-6"), "o1-preview");
        assert_eq!(m.map_model("claude-haiku-4-5-20251001"), "o1-mini");
    }

    // --- Vertex / BackendKind tests ---

    #[test]
    fn vertex_url_construction() {
        let url = format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/endpoints/openapi",
            "us-central1", "my-project", "us-central1"
        );
        assert_eq!(
            url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi"
        );
    }

    #[test]
    fn vertex_base_url_passes_ssrf() {
        let url = "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi";
        assert!(validate_base_url(url).is_ok());
    }

    #[test]
    fn vertex_model_defaults() {
        let m = ModelMapping::from_env_with_defaults("gemini-2.5-pro", "gemini-2.5-flash");
        // When BIG_MODEL/SMALL_MODEL env vars are not set, uses Vertex defaults
        // (This test works because env vars are unlikely to be set in test environment)
        assert_eq!(m.map_model("claude-sonnet-4-6"), "gemini-2.5-pro");
        assert_eq!(m.map_model("claude-haiku-4-5"), "gemini-2.5-flash");
    }

    #[test]
    fn backend_auth_debug_redacts() {
        let bearer = BackendAuth::BearerToken("secret-token".into());
        let debug = format!("{:?}", bearer);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("secret-token"));

        let api_key = BackendAuth::GoogleApiKey("secret-key".into());
        let debug = format!("{:?}", api_key);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("secret-key"));
    }

    // --- MultiConfig TOML parsing tests ---

    #[test]
    fn multi_config_parses_openai_backend() {
        let toml = r#"
            listen_port = 4000
            default_backend = "openai"

            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
            big_model = "gpt-4o"
            small_model = "gpt-4o-mini"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.listen_port, 4000);
        assert_eq!(mc.default_backend, "openai");
        assert_eq!(mc.backends.len(), 1);
        let bc = &mc.backends["openai"];
        assert_eq!(bc.kind, BackendKind::OpenAI);
        assert_eq!(bc.api_key, "sk-test");
        assert_eq!(bc.model_mapping.big_model, "gpt-4o");
        assert_eq!(bc.model_mapping.small_model, "gpt-4o-mini");
    }

    #[test]
    fn multi_config_parses_multiple_backends() {
        let toml = r#"
            default_backend = "openai"

            [backends.openai]
            kind = "openai"
            api_key = "sk-test"

            [backends.gemini]
            kind = "gemini"
            api_key = "AIzaSy"

            [backends.claude]
            kind = "anthropic"
            api_key = "sk-ant-test"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.backends.len(), 3);
        assert_eq!(mc.backends["openai"].kind, BackendKind::OpenAI);
        assert_eq!(mc.backends["gemini"].kind, BackendKind::Gemini);
        assert_eq!(mc.backends["claude"].kind, BackendKind::Anthropic);
    }

    #[test]
    fn multi_config_defaults_first_backend_as_default() {
        let toml = r#"
            [backends.gemini]
            kind = "gemini"
            api_key = "AIzaSy"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.default_backend, "gemini");
    }

    #[test]
    fn multi_config_defaults_listen_port() {
        let toml = r#"
            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.listen_port, 3000);
    }

    #[test]
    fn multi_config_openai_defaults_base_url() {
        let toml = r#"
            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.backends["openai"].base_url, "https://api.openai.com");
    }

    #[test]
    fn multi_config_anthropic_defaults_base_url() {
        let toml = r#"
            [backends.claude]
            kind = "anthropic"
            api_key = "sk-ant-test"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.backends["claude"].base_url, "https://api.anthropic.com");
    }

    #[test]
    fn multi_config_custom_base_url() {
        let toml = r#"
            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
            base_url = "https://custom.openai.example.com"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(
            mc.backends["openai"].base_url,
            "https://custom.openai.example.com"
        );
    }

    #[test]
    fn multi_config_api_format_responses() {
        let toml = r#"
            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
            api_format = "responses"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.backends["openai"].api_format, OpenAIApiFormat::Responses);
    }

    #[test]
    #[should_panic(expected = "must define at least one backend")]
    fn multi_config_panics_no_backends() {
        let toml = r#"
            listen_port = 3000
        "#;
        MultiConfig::from_toml_str(toml);
    }

    #[test]
    #[should_panic(expected = "not found in configured backends")]
    fn multi_config_panics_invalid_default() {
        let toml = r#"
            default_backend = "nonexistent"

            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
        "#;
        MultiConfig::from_toml_str(toml);
    }

    #[test]
    #[should_panic(expected = "unknown backend kind")]
    fn multi_config_panics_unknown_kind() {
        let toml = r#"
            [backends.foo]
            kind = "unknown_provider"
            api_key = "test"
        "#;
        MultiConfig::from_toml_str(toml);
    }

    #[test]
    #[should_panic(expected = "api_key is required for gemini")]
    fn multi_config_panics_gemini_no_key() {
        let toml = r#"
            [backends.gemini]
            kind = "gemini"
        "#;
        MultiConfig::from_toml_str(toml);
    }

    #[test]
    #[should_panic(expected = "api_key is required for anthropic")]
    fn multi_config_panics_anthropic_no_key() {
        let toml = r#"
            [backends.claude]
            kind = "anthropic"
        "#;
        MultiConfig::from_toml_str(toml);
    }

    #[test]
    fn resolve_env_value_inline() {
        assert_eq!(resolve_env_value("my-key").unwrap(), "my-key");
    }

    #[test]
    fn resolve_env_value_from_env() {
        std::env::set_var("TEST_RESOLVE_KEY_12345", "resolved-value");
        assert_eq!(
            resolve_env_value("env:TEST_RESOLVE_KEY_12345").unwrap(),
            "resolved-value"
        );
        std::env::remove_var("TEST_RESOLVE_KEY_12345");
    }

    #[test]
    fn resolve_env_value_missing_env() {
        let err = resolve_env_value("env:NONEXISTENT_VAR_99999").unwrap_err();
        assert!(err.contains("not set"));
    }

    #[test]
    fn multi_config_env_prefix_resolves() {
        std::env::set_var("TEST_OPENAI_KEY_TOML", "sk-from-env");
        let toml = r#"
            [backends.openai]
            kind = "openai"
            api_key = "env:TEST_OPENAI_KEY_TOML"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert_eq!(mc.backends["openai"].api_key, "sk-from-env");
        std::env::remove_var("TEST_OPENAI_KEY_TOML");
    }

    #[test]
    fn multi_config_log_bodies() {
        let toml = r#"
            log_bodies = true

            [backends.openai]
            kind = "openai"
            api_key = "sk-test"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        assert!(mc.log_bodies);
        assert!(mc.backends["openai"].log_bodies);
    }

    #[test]
    fn multi_config_gemini_defaults() {
        let toml = r#"
            [backends.gemini]
            kind = "gemini"
            api_key = "AIzaSy"
        "#;
        let mc = MultiConfig::from_toml_str(toml);
        let bc = &mc.backends["gemini"];
        assert_eq!(bc.model_mapping.big_model, "gemini-2.5-pro");
        assert_eq!(bc.model_mapping.small_model, "gemini-2.5-flash");
        // /openai is appended to route through Gemini's OpenAI-compatible endpoint
        assert_eq!(
            bc.base_url,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
    }
}
