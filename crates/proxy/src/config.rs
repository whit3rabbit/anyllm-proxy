use indexmap::IndexMap;
use serde::Deserialize;
use std::fmt;
use std::net::IpAddr;
use url::Url;

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

impl Config {
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
    pub fn from_env() -> Self {
        Self::from_env_with_defaults("gpt-4o", "gpt-4o-mini")
    }

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

/// Optional mTLS configuration for the backend connection.
/// Stores raw certificate bytes so Config remains Clone.
/// Validated at construction time: bad certs cause startup panic.
#[derive(Clone, Default)]
pub struct TlsConfig {
    /// Raw PKCS#12 bytes and password for client certificate authentication.
    pub p12_identity: Option<(Vec<u8>, String)>,
    /// Raw PEM bytes for additional CA certificate to trust.
    pub ca_cert_pem: Option<Vec<u8>>,
}

impl fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
            .field(
                "p12_identity",
                &self.p12_identity.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "ca_cert_pem",
                &self
                    .ca_cert_pem
                    .as_ref()
                    .map(|b| format!("{} bytes", b.len())),
            )
            .finish()
    }
}

impl TlsConfig {
    /// Load and validate TLS config from file paths.
    /// Panics on invalid/missing files or wrong password.
    pub fn load(p12_path: Option<&str>, p12_password: Option<&str>, ca_path: Option<&str>) -> Self {
        let p12_identity = match (p12_path, p12_password) {
            (Some(path), Some(password)) => {
                let bytes = std::fs::read(path)
                    .unwrap_or_else(|e| panic!("failed to read P12 file '{}': {}", path, e));

                // Validate the P12 parses correctly with the given password
                reqwest::Identity::from_pkcs12_der(&bytes, password).unwrap_or_else(|e| {
                    panic!(
                        "invalid P12 file '{}' (wrong password or corrupt file): {}",
                        path, e
                    )
                });

                tracing::info!(path = %path, "loaded client certificate (P12)");
                Some((bytes, password.to_string()))
            }
            (Some(_), None) => {
                panic!("TLS_CLIENT_CERT_P12 is set but TLS_CLIENT_CERT_PASSWORD is missing");
            }
            (None, Some(_)) => {
                tracing::warn!(
                    "TLS_CLIENT_CERT_PASSWORD is set but TLS_CLIENT_CERT_P12 is not, ignoring"
                );
                None
            }
            (None, None) => None,
        };

        let ca_cert_pem = ca_path.map(|path| {
            let bytes = std::fs::read(path)
                .unwrap_or_else(|e| panic!("failed to read CA cert file '{}': {}", path, e));

            // Validate the PEM parses as a certificate
            reqwest::Certificate::from_pem(&bytes)
                .unwrap_or_else(|e| panic!("invalid CA certificate '{}': {}", path, e));

            tracing::info!(path = %path, "loaded custom CA certificate");
            bytes
        });

        Self {
            p12_identity,
            ca_cert_pem,
        }
    }

    /// Load from environment variables.
    pub fn from_env() -> Self {
        let p12_path = std::env::var("TLS_CLIENT_CERT_P12").ok();
        let p12_password = std::env::var("TLS_CLIENT_CERT_PASSWORD").ok();
        let ca_path = std::env::var("TLS_CA_CERT").ok();
        Self::load(
            p12_path.as_deref(),
            p12_password.as_deref(),
            ca_path.as_deref(),
        )
    }
}

/// Validate that a base URL is safe to use as an upstream target.
/// Rejects non-http(s) schemes, private/loopback IPs, and link-local addresses.
/// For domain names, also resolves DNS and validates all resolved IPs to prevent
/// DNS rebinding attacks (where a domain initially resolves to a public IP but
/// later changes to a private/metadata IP).
pub fn validate_base_url(raw: &str) -> Result<(), String> {
    let parsed = Url::parse(raw).map_err(|e| format!("invalid URL: {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme '{other}' not allowed, use http or https")),
    }

    match parsed.host() {
        None => return Err("URL has no host".to_string()),
        Some(url::Host::Ipv4(v4)) => {
            let ip = IpAddr::V4(v4);
            if is_private_ip(ip) {
                return Err(format!("private/loopback IP {ip} not allowed"));
            }
        }
        Some(url::Host::Ipv6(v6)) => {
            let ip = IpAddr::V6(v6);
            if is_private_ip(ip) {
                return Err(format!("private/loopback IP {ip} not allowed"));
            }
        }
        Some(url::Host::Domain(domain)) => {
            let lower = domain.to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".localhost")
                || lower == "metadata.google.internal"
                || lower.ends_with(".internal")
            {
                return Err(format!("hostname '{domain}' not allowed"));
            }

            // Resolve DNS at startup and validate all resolved IPs.
            // This catches domains that currently resolve to private/metadata IPs.
            // Note: does not prevent post-startup DNS rebinding; for full protection,
            // restrict outbound traffic at the network level.
            let port = parsed
                .port()
                .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
            let lookup = format!("{domain}:{port}");
            if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&lookup) {
                for addr in addrs {
                    if is_private_ip(addr.ip()) {
                        return Err(format!(
                            "hostname '{domain}' resolves to private/loopback IP {}, not allowed",
                            addr.ip()
                        ));
                    }
                }
            }
            // If DNS resolution fails, allow it (the domain may not be resolvable
            // in the build/test environment but will work at runtime).
        }
    }

    Ok(())
}

/// Returns true for loopback, private (RFC 1918), link-local, and
/// cloud metadata IPs (169.254.169.254).
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                // Cloud metadata endpoint
                || v4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified()
            // ::1, ::, and IPv4-mapped private addresses
            || matches!(v6.to_ipv4_mapped(), Some(v4) if is_private_ip(IpAddr::V4(v4)))
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-backend configuration
// ---------------------------------------------------------------------------

/// Resolve a config value that may reference an env var via `env:VAR_NAME` prefix.
/// Returns the raw value if no prefix, or the env var contents if prefixed.
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
    pub kind: BackendKind,
    pub api_key: String,
    pub base_url: String,
    pub api_format: OpenAIApiFormat,
    pub model_mapping: ModelMapping,
    pub tls: TlsConfig,
    pub backend_auth: BackendAuth,
    pub log_bodies: bool,
}

/// Top-level multi-backend configuration.
#[derive(Debug, Clone)]
pub struct MultiConfig {
    pub listen_port: u16,
    pub log_bodies: bool,
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

        let bc = BackendConfig {
            kind: config.backend.clone(),
            api_key: config.openai_api_key.clone(),
            base_url: config.openai_base_url.clone(),
            api_format: config.openai_api_format.clone(),
            model_mapping: config.model_mapping.clone(),
            tls: config.tls.clone(),
            backend_auth: config.backend_auth.clone(),
            log_bodies: config.log_bodies,
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
                (base_url, auth, mm, OpenAIApiFormat::Chat)
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_https_url() {
        assert!(validate_base_url("https://api.openai.com").is_ok());
    }

    #[test]
    fn valid_http_url() {
        assert!(validate_base_url("http://my-proxy.example.com").is_ok());
    }

    #[test]
    fn rejects_ftp_scheme() {
        let err = validate_base_url("ftp://evil.com").unwrap_err();
        assert!(err.contains("scheme"));
    }

    #[test]
    fn rejects_localhost() {
        let err = validate_base_url("http://localhost:8080").unwrap_err();
        assert!(err.contains("not allowed"));
    }

    #[test]
    fn rejects_loopback_ip() {
        let err = validate_base_url("http://127.0.0.1:8080").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_10_range() {
        let err = validate_base_url("http://10.0.0.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_172_range() {
        let err = validate_base_url("http://172.16.0.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_192_range() {
        let err = validate_base_url("http://192.168.1.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_cloud_metadata() {
        let err = validate_base_url("http://169.254.169.254").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_metadata_hostname() {
        let err = validate_base_url("http://metadata.google.internal").unwrap_err();
        assert!(err.contains("not allowed"));
    }

    #[test]
    fn rejects_ipv6_loopback() {
        let err = validate_base_url("http://[::1]:8080").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_unspecified() {
        let err = validate_base_url("http://0.0.0.0").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_invalid_url() {
        let err = validate_base_url("not a url").unwrap_err();
        assert!(err.contains("invalid URL"));
    }

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

    // --- TlsConfig tests ---

    /// Path to test fixtures relative to the workspace root.
    fn fixture_path(name: &str) -> String {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{manifest}/tests/fixtures/tls/{name}")
    }

    #[test]
    fn tls_config_none_when_no_paths() {
        let tls = TlsConfig::load(None, None, None);
        assert!(tls.p12_identity.is_none());
        assert!(tls.ca_cert_pem.is_none());
    }

    #[test]
    #[should_panic(expected = "TLS_CLIENT_CERT_PASSWORD is missing")]
    fn tls_config_panics_missing_password() {
        TlsConfig::load(Some("/any/path.p12"), None, None);
    }

    #[test]
    #[should_panic(expected = "failed to read P12 file")]
    fn tls_config_panics_missing_p12_file() {
        TlsConfig::load(Some("/nonexistent/file.p12"), Some("pass"), None);
    }

    #[test]
    fn tls_config_loads_valid_p12() {
        let path = fixture_path("test-client.p12");
        let tls = TlsConfig::load(Some(&path), Some("test"), None);
        assert!(tls.p12_identity.is_some());
        assert!(tls.ca_cert_pem.is_none());
    }

    #[test]
    fn tls_config_loads_valid_ca() {
        let path = fixture_path("test-ca.pem");
        let tls = TlsConfig::load(None, None, Some(&path));
        assert!(tls.p12_identity.is_none());
        assert!(tls.ca_cert_pem.is_some());
    }

    #[test]
    fn tls_config_loads_both() {
        let p12 = fixture_path("test-client.p12");
        let ca = fixture_path("test-ca.pem");
        let tls = TlsConfig::load(Some(&p12), Some("test"), Some(&ca));
        assert!(tls.p12_identity.is_some());
        assert!(tls.ca_cert_pem.is_some());
    }

    #[test]
    fn tls_config_debug_redacts_password() {
        let p12 = fixture_path("test-client.p12");
        let tls = TlsConfig::load(Some(&p12), Some("test"), None);
        let debug = format!("{:?}", tls);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("test"));
    }

    #[test]
    #[should_panic(expected = "invalid P12 file")]
    fn tls_config_panics_wrong_password() {
        let path = fixture_path("test-client.p12");
        TlsConfig::load(Some(&path), Some("wrong-password"), None);
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
        assert_eq!(
            bc.base_url,
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }
}
