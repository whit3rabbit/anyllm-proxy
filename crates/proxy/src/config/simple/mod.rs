//! Simple native YAML config format for anyllm-proxy.
//!
//! Activated when the config file contains a top-level `models:` key
//! (as opposed to LiteLLM's `model_list:`).

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::Deserialize;

use super::litellm::parse_routing_strategy_str;
use super::model_router::{Deployment, ModelRouter, RoutingStrategy};
use super::{
    validate_base_url, BackendAuth, BackendConfig, BackendKind, ModelMapping, MultiConfig,
    OpenAIApiFormat, TlsConfig,
};

/// Top-level simple config document.
#[derive(Debug, Deserialize)]
pub struct SimpleConfig {
    /// Routing strategy for all models. Case-insensitive.
    /// Accepted values: round-robin, least-busy, latency-based, weighted, cost-based.
    /// Default: round-robin.
    #[serde(default)]
    pub routing_strategy: Option<String>,
    /// Proxy listen port. Default: 3000.
    #[serde(default)]
    pub listen_port: Option<u16>,
    /// Log request/response bodies at debug level. Default: false.
    #[serde(default)]
    pub log_bodies: Option<bool>,
    /// Redact detected secrets from upstream JSON/text request payloads.
    #[serde(default)]
    pub redact_secrets: Option<bool>,
    /// List of model deployments.
    #[serde(default)]
    pub models: Vec<SimpleModelEntry>,
    #[serde(default)]
    pub tool_execution: Option<ToolExecutionConfig>,
    #[serde(default)]
    pub builtin_tools: Option<HashMap<String, BuiltinToolConfig>>,
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServerConfig>>,
}

/// A model entry: either a string shorthand or a full struct.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SimpleModelEntry {
    /// String shorthand: "model-name" or "provider/model-name".
    Shorthand(String),
    /// Full form with all fields. Boxed to reduce enum size.
    Full(Box<SimpleModelFull>),
}

/// Full model entry with all optional fields.
#[derive(Debug, Deserialize)]
pub struct SimpleModelFull {
    /// Virtual model name clients send in requests. Defaults to `model` if omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// Actual model name forwarded to the backend.
    pub model: String,
    /// Backend provider. Default: "openai".
    #[serde(default)]
    pub provider: Option<String>,
    /// Static weight for weighted routing. Default: 1.
    #[serde(default)]
    pub weight: Option<u32>,
    /// Per-deployment requests-per-minute limit.
    #[serde(default)]
    pub rpm: Option<u32>,
    /// Per-deployment tokens-per-minute limit.
    #[serde(default)]
    pub tpm: Option<u64>,
    /// API key override. When absent, falls back to the standard env var for the provider.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL override. When absent, uses the provider default.
    #[serde(default)]
    pub api_base: Option<String>,
    // Azure-specific
    #[serde(default)]
    pub deployment: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    // Vertex-specific
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    // Bedrock-specific
    #[serde(default)]
    pub aws_region: Option<String>,
    #[serde(default)]
    pub aws_access_key_id: Option<String>,
    #[serde(default)]
    pub aws_secret_access_key: Option<String>,
}

/// Tool execution loop configuration.
#[derive(Debug, Deserialize)]
pub struct ToolExecutionConfig {
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default)]
    pub total_timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_tool_calls_per_turn: Option<usize>,
}

/// Configuration for a single builtin tool.
#[derive(Debug, Deserialize)]
pub struct BuiltinToolConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// For read_file: restrict reads to files under these absolute directory paths.
    /// If empty or absent, read_file is disabled. Use ["/"] to explicitly allow unrestricted reads.
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// MCP server configuration entry.
#[derive(Debug, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub policy: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsed result + public parser
// ---------------------------------------------------------------------------

/// Tool-related config extracted from SimpleConfig, passed up to main.rs
/// so it can build ToolEngineState without re-parsing the config file.
#[derive(Debug)]
pub struct ToolStartupConfig {
    pub tool_execution: Option<ToolExecutionConfig>,
    pub builtin_tools: Option<HashMap<String, BuiltinToolConfig>>,
    pub mcp_servers: Option<Vec<McpServerConfig>>,
}

impl ToolStartupConfig {
    /// Returns true when at least one tool-related section was present in the config.
    /// Used to decide whether to construct a ToolEngineState at all.
    pub fn has_any(&self) -> bool {
        self.tool_execution.is_some() || self.builtin_tools.is_some() || self.mcp_servers.is_some()
    }
}

/// Result from parsing a simple YAML config file.
pub struct SimpleParsed {
    pub multi_config: MultiConfig,
    pub router: ModelRouter,
    /// Tool-related sections extracted from the config. None-valued when no tool
    /// sections were present (callers should check `has_any()` before using).
    pub tool_config: ToolStartupConfig,
}

/// Parse a simple YAML config string and produce a `MultiConfig + ModelRouter`.
///
/// # Panics
/// On invalid YAML, empty model list, or unresolvable required values.
pub fn parse_simple_yaml(yaml: &str) -> SimpleParsed {
    let config: SimpleConfig =
        serde_yaml::from_str(yaml).unwrap_or_else(|e| panic!("invalid simple config YAML: {e}"));

    if config.models.is_empty() {
        panic!("simple config must define at least one model");
    }

    let listen_port = config
        .listen_port
        .or_else(|| {
            std::env::var("LISTEN_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(3000);

    let log_bodies = config.log_bodies.unwrap_or_else(|| {
        std::env::var("LOG_BODIES")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    });
    let redact_secrets = config.redact_secrets.unwrap_or_else(|| {
        std::env::var("REDACT_SECRETS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    });

    let tls = TlsConfig::from_env();

    #[derive(Hash, PartialEq, Eq)]
    struct BackendKey {
        kind: String,
        base_url: String,
        api_key_hash: u64,
    }

    fn hash_str(s: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.hash(&mut h);
        h.finish()
    }

    struct DepSpec {
        backend_name: String,
        actual_model: String,
        rpm: Option<u32>,
        tpm: Option<u64>,
        weight: u32,
    }

    let mut backend_map: HashMap<BackendKey, (String, BackendConfig)> = HashMap::new();
    let mut backend_counter = 0u32;
    let mut model_deployments: HashMap<String, Vec<DepSpec>> = HashMap::new();

    for entry in &config.models {
        let norm = normalize_entry(entry);
        let kind = parse_kind(&norm.provider);
        let api_key = super::sanitize_api_key(
            &norm
                .api_key
                .clone()
                .unwrap_or_else(|| default_api_key_for_provider(&norm.provider, &kind)),
        );
        let base_url = if kind == BackendKind::AzureOpenAI {
            // Azure always builds a full deployment URL (api_base or env var + deployment + version).
            default_base_url(&kind, &norm)
        } else {
            norm.api_base
                .clone()
                .unwrap_or_else(|| default_base_url(&kind, &norm))
        };

        if kind != BackendKind::Bedrock {
            if let Err(e) = validate_base_url(&base_url) {
                panic!("model '{}' base_url rejected: {e}", norm.virtual_name);
            }
        }

        let bk = BackendKey {
            kind: format!("{kind:?}"),
            base_url: base_url.clone(),
            api_key_hash: hash_str(&api_key),
        };

        let backend_name = if let Some((name, _)) = backend_map.get(&bk) {
            name.clone()
        } else {
            let name = format!("simple_{backend_counter}");
            backend_counter += 1;
            let bc =
                build_backend_config(&name, &kind, &api_key, &base_url, &norm, &tls, log_bodies);
            backend_map.insert(bk, (name.clone(), bc));
            name
        };

        model_deployments
            .entry(norm.virtual_name.clone())
            .or_default()
            .push(DepSpec {
                backend_name,
                actual_model: norm.actual_model.clone(),
                rpm: norm.rpm,
                tpm: norm.tpm,
                weight: norm.weight.unwrap_or(1),
            });
    }

    let mut backends = IndexMap::new();
    for (name, bc) in backend_map.values() {
        backends.insert(name.clone(), bc.clone());
    }
    let default_backend = backends
        .keys()
        .next()
        .cloned()
        .expect("at least one backend");
    let multi = MultiConfig {
        listen_port,
        log_bodies,
        redact_secrets,
        default_backend,
        backends,
        expose_degradation_warnings: false,
    };

    let strategy: RoutingStrategy = config
        .routing_strategy
        .as_deref()
        .map(parse_routing_strategy_str)
        .unwrap_or_default();

    let mut routes: HashMap<String, Vec<Arc<Deployment>>> = HashMap::new();
    for (virtual_name, specs) in model_deployments {
        let deployments = specs
            .into_iter()
            .map(|s| {
                Arc::new(Deployment::with_weight(
                    s.backend_name,
                    s.actual_model,
                    s.rpm,
                    s.tpm,
                    s.weight,
                ))
            })
            .collect();
        routes.insert(virtual_name, deployments);
    }
    let router = ModelRouter::with_strategy(routes, strategy);

    SimpleParsed {
        multi_config: multi,
        router,
        tool_config: ToolStartupConfig {
            tool_execution: config.tool_execution,
            builtin_tools: config.builtin_tools,
            mcp_servers: config.mcp_servers,
        },
    }
}

// ---------------------------------------------------------------------------
// SimpleConfig methods
// ---------------------------------------------------------------------------

impl SimpleConfig {
    /// Convert the YAML tool configuration into the in-memory `ToolExecutionPolicy` + `LoopConfig`
    /// used at request time. Called once at startup after the config file is parsed.
    pub fn build_tool_config(
        &self,
    ) -> (crate::tools::ToolExecutionPolicy, crate::tools::LoopConfig) {
        use crate::tools::policy::{PolicyAction, PolicyRule};

        let mut rules = Vec::new();

        // Builtin tool rules.
        if let Some(ref builtins) = self.builtin_tools {
            for (name, cfg) in builtins {
                if !cfg.enabled {
                    continue;
                }
                let action = match cfg.policy.as_deref() {
                    Some("allow") => PolicyAction::Allow,
                    Some("deny") => PolicyAction::Deny,
                    _ => PolicyAction::PassThrough,
                };
                // Warn loudly when execute_bash is set to Allow: it executes
                // arbitrary OS commands as the proxy process user. Operators
                // should only enable this inside a sandboxed environment.
                if name == "execute_bash" && action == PolicyAction::Allow {
                    tracing::warn!(
                        "execute_bash policy is Allow: the LLM can execute arbitrary OS \
                         commands as the proxy process user. Only enable this inside an \
                         isolated sandbox (seccomp, read-only rootfs, network isolation)."
                    );
                }
                rules.push(PolicyRule {
                    tool_name: name.clone(),
                    action,
                    timeout: cfg.timeout_secs.map(std::time::Duration::from_secs),
                    max_concurrency: None,
                });
            }
        }

        // MCP server rules: glob rule per server for prefixed tool names.
        if let Some(ref servers) = self.mcp_servers {
            for server in servers {
                let action = match server.policy.as_deref() {
                    Some("allow") => PolicyAction::Allow,
                    Some("deny") => PolicyAction::Deny,
                    _ => PolicyAction::PassThrough,
                };
                rules.push(PolicyRule {
                    tool_name: format!("mcp_{}_*", server.name),
                    action,
                    timeout: None,
                    max_concurrency: None,
                });
            }
        }

        let policy = crate::tools::ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules,
        };

        let loop_config = if let Some(ref te) = self.tool_execution {
            crate::tools::LoopConfig {
                max_iterations: te.max_iterations.unwrap_or(1),
                tool_timeout: std::time::Duration::from_secs(te.tool_timeout_secs.unwrap_or(30)),
                total_timeout: std::time::Duration::from_secs(te.total_timeout_secs.unwrap_or(300)),
                max_tool_calls_per_turn: te.max_tool_calls_per_turn.unwrap_or(16),
            }
        } else {
            crate::tools::LoopConfig::default()
        };

        (policy, loop_config)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct NormalizedEntry {
    virtual_name: String,
    provider: String,
    actual_model: String,
    api_key: Option<String>,
    api_base: Option<String>,
    weight: Option<u32>,
    rpm: Option<u32>,
    tpm: Option<u64>,
    deployment: Option<String>,
    api_version: Option<String>,
    project: Option<String>,
    region: Option<String>,
    aws_region: Option<String>,
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
}

fn normalize_entry(entry: &SimpleModelEntry) -> NormalizedEntry {
    match entry {
        SimpleModelEntry::Shorthand(s) => {
            let (provider, model) = s
                .split_once('/')
                .map(|(p, m)| (p.to_string(), m.to_string()))
                .unwrap_or_else(|| ("openai".to_string(), s.clone()));
            NormalizedEntry {
                virtual_name: model.clone(),
                provider,
                actual_model: model,
                api_key: None,
                api_base: None,
                weight: None,
                rpm: None,
                tpm: None,
                deployment: None,
                api_version: None,
                project: None,
                region: None,
                aws_region: None,
                aws_access_key_id: None,
                aws_secret_access_key: None,
            }
        }
        SimpleModelEntry::Full(f) => {
            let provider = f.provider.clone().unwrap_or_else(|| "openai".to_string());
            let virtual_name = f.name.clone().unwrap_or_else(|| f.model.clone());
            NormalizedEntry {
                virtual_name,
                provider,
                actual_model: f.model.clone(),
                api_key: f.api_key.clone(),
                api_base: f.api_base.clone(),
                weight: f.weight,
                rpm: f.rpm,
                tpm: f.tpm,
                deployment: f.deployment.clone(),
                api_version: f.api_version.clone(),
                project: f.project.clone(),
                region: f.region.clone(),
                aws_region: f.aws_region.clone(),
                aws_access_key_id: f.aws_access_key_id.clone(),
                aws_secret_access_key: f.aws_secret_access_key.clone(),
            }
        }
    }
}

fn parse_kind(provider: &str) -> BackendKind {
    match provider.to_ascii_lowercase().as_str() {
        "openai" => BackendKind::OpenAI,
        "azure" => BackendKind::AzureOpenAI,
        "vertex_ai" | "vertex" => BackendKind::Vertex,
        "gemini" => BackendKind::Gemini,
        "anthropic" => BackendKind::Anthropic,
        "bedrock" => BackendKind::Bedrock,
        other => {
            tracing::warn!(provider = %other, "unknown provider, treating as openai-compatible");
            BackendKind::OpenAI
        }
    }
}

fn default_api_key_for_provider(provider: &str, kind: &BackendKind) -> String {
    let var = match kind {
        BackendKind::OpenAI => "OPENAI_API_KEY",
        BackendKind::Anthropic => "ANTHROPIC_API_KEY",
        BackendKind::Gemini => "GEMINI_API_KEY",
        BackendKind::Vertex => {
            return std::env::var("VERTEX_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_ACCESS_TOKEN"))
                .unwrap_or_default();
        }
        BackendKind::AzureOpenAI => "AZURE_OPENAI_API_KEY",
        BackendKind::Bedrock => return String::new(),
    };
    std::env::var(var).unwrap_or_else(|_| {
        tracing::warn!(
            provider = %provider,
            env_var = %var,
            "provider API key env var not set; backend calls will likely fail"
        );
        String::new()
    })
}

fn default_base_url(kind: &BackendKind, entry: &NormalizedEntry) -> String {
    match kind {
        BackendKind::OpenAI => {
            let url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string());
            super::strip_v1_suffix(&url).to_string()
        }
        BackendKind::Gemini => {
            let base = std::env::var("GEMINI_BASE_URL")
                .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string());
            format!("{base}/openai")
        }
        BackendKind::Anthropic => "https://api.anthropic.com".to_string(),
        BackendKind::Vertex => {
            let project = entry
                .project
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::var("VERTEX_PROJECT").expect(
                        "project field (or VERTEX_PROJECT env var) required for vertex provider",
                    )
                });
            let region = entry
                .region
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::var("VERTEX_REGION").expect(
                        "region field (or VERTEX_REGION env var) required for vertex provider",
                    )
                });
            format!(
                "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/endpoints/openapi"
            )
        }
        BackendKind::AzureOpenAI => {
            let endpoint = entry
                .api_base
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::var("AZURE_OPENAI_ENDPOINT").expect(
                        "api_base field (or AZURE_OPENAI_ENDPOINT env var) required for azure provider",
                    )
                });
            // Guard against double-appending if user provided a full deployment URL.
            if endpoint.contains("/openai/deployments/") {
                endpoint
            } else {
                let dep = entry.deployment.as_deref().unwrap_or("chat");
                let version = entry.api_version.as_deref().unwrap_or("2024-10-21");
                format!(
                    "{}/openai/deployments/{dep}/chat/completions?api-version={version}",
                    endpoint.trim_end_matches('/')
                )
            }
        }
        BackendKind::Bedrock => {
            // Bedrock doesn't use a URL — the region string is stored in base_url
            // and used by the Bedrock client directly for SigV4 endpoint construction.
            entry
                .aws_region
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string())
                })
        }
    }
}

fn build_backend_config(
    name: &str,
    kind: &BackendKind,
    api_key: &str,
    base_url: &str,
    entry: &NormalizedEntry,
    tls: &TlsConfig,
    log_bodies: bool,
) -> BackendConfig {
    let backend_auth = match kind {
        BackendKind::AzureOpenAI => BackendAuth::AzureApiKey(api_key.to_string()),
        BackendKind::Gemini | BackendKind::Vertex => BackendAuth::GoogleApiKey(api_key.to_string()),
        _ => BackendAuth::BearerToken(api_key.to_string()),
    };

    let bedrock_credentials = if *kind == BackendKind::Bedrock {
        let access_key = entry
            .aws_access_key_id
            .as_deref()
            .map(|s| s.to_string())
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .unwrap_or_else(|| panic!("backend '{name}': aws_access_key_id required for bedrock"));

        let secret_key = entry
            .aws_secret_access_key
            .as_deref()
            .map(|s| s.to_string())
            .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
            .unwrap_or_else(|| {
                panic!("backend '{name}': aws_secret_access_key required for bedrock")
            });

        Some(aws_credential_types::Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "simple-config",
        ))
    } else {
        None
    };

    BackendConfig {
        kind: kind.clone(),
        api_key: api_key.to_string(),
        base_url: base_url.to_string(),
        api_format: OpenAIApiFormat::Chat,
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: tls.clone(),
        backend_auth,
        log_bodies,
        omit_stream_options: false,
        stream_timeout_secs: 900,
        bedrock_credentials,
    }
}

#[cfg(test)]
mod tests;
