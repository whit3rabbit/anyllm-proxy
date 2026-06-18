/// LiteLLM config.yaml parser.
///
/// Accepts LiteLLM's YAML config format (model_list, litellm_settings,
/// router_settings, general_settings) and converts it to anyllm-proxy's
/// MultiConfig + ModelRouter.
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::Deserialize;

use super::model_router::{Deployment, ModelRouter, RoutingStrategy};
use super::{
    resolve_env_value, validate_base_url, BackendAuth, BackendConfig, BackendKind, ModelMapping,
    MultiConfig, OpenAIApiFormat, TlsConfig,
};

// ---- Serde structs for LiteLLM config.yaml ----

/// Root structure of a LiteLLM `config.yaml` file.
#[derive(Deserialize)]
pub(crate) struct LiteLLMConfig {
    #[serde(default)]
    model_list: Vec<LiteLLMModelEntry>,
    #[serde(default)]
    litellm_settings: Option<LiteLLMSettings>,
    #[serde(default)]
    router_settings: Option<RouterSettings>,
    #[serde(default)]
    general_settings: Option<GeneralSettings>,
}

#[derive(Deserialize)]
struct LiteLLMModelEntry {
    model_name: String,
    litellm_params: LiteLLMParams,
}

#[derive(Deserialize)]
struct LiteLLMParams {
    model: String,
    api_base: Option<String>,
    api_key: Option<String>,
    rpm: Option<u32>,
    tpm: Option<u64>,
    weight: Option<u32>,
    // Azure-specific
    api_version: Option<String>,
    // Bedrock-specific
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
    aws_region_name: Option<String>,
    // Catch unknown fields silently (LiteLLM has many we don't support).
    #[serde(flatten)]
    _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct LiteLLMSettings {
    #[serde(default)]
    num_retries: Option<u32>,
    #[serde(default)]
    request_timeout: Option<u64>,
    #[serde(default)]
    callbacks: Vec<String>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RouterSettings {
    #[serde(default)]
    routing_strategy: Option<String>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, serde_json::Value>,
}

/// Map LiteLLM routing_strategy string to our enum.
pub(crate) fn parse_routing_strategy_str(s: &str) -> RoutingStrategy {
    match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "simple-shuffle" | "round-robin" => RoutingStrategy::RoundRobin,
        "least-busy" => RoutingStrategy::LeastBusy,
        "latency-based-routing" | "latency-based" => RoutingStrategy::LatencyBased,
        "usage-based-routing" | "usage-based" => RoutingStrategy::LeastBusy,
        "weighted" => RoutingStrategy::Weighted,
        "cost-based" => RoutingStrategy::CostBased,
        other => {
            tracing::warn!(
                strategy = %other,
                "unknown routing_strategy, falling back to round-robin"
            );
            RoutingStrategy::RoundRobin
        }
    }
}

#[derive(Deserialize)]
struct GeneralSettings {
    master_key: Option<String>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, serde_json::Value>,
}

// ---- Provider parsing ----

/// Parse LiteLLM's "provider/model_name" format.
/// No prefix defaults to OpenAI (matches LiteLLM behavior).
/// Returns (kind, model_name, stub_provider) where stub_provider is set for
/// registry-resolved OpenAI-compatible providers so callers can use their default URL.
fn parse_provider_model(
    model: &str,
) -> (
    BackendKind,
    String,
    Option<&'static anyllm_providers::ProviderDef>,
) {
    let (provider, model_name) = model.split_once('/').unwrap_or(("openai", model));
    let mut stub_provider: Option<&'static anyllm_providers::ProviderDef> = None;
    let kind = match provider.to_ascii_lowercase().as_str() {
        "openai" => BackendKind::OpenAI,
        "azure" => BackendKind::AzureOpenAI,
        "vertex_ai" | "vertex" => BackendKind::Vertex,
        "gemini" => BackendKind::Gemini,
        "anthropic" => {
            stub_provider = anyllm_providers::get_provider("anthropic");
            BackendKind::Anthropic
        }
        "bedrock" => BackendKind::Bedrock,
        other => {
            // Try the provider registry for known OpenAI-compatible providers
            // (e.g. "groq", "together_ai", "mistral", etc.)
            let prefix_with_slash = format!("{other}/");
            if let Some(p) = anyllm_providers::find_by_litellm_prefix(&prefix_with_slash) {
                let resolved = match anyllm_providers::resolve_backend(p.id) {
                    Some(("openai", _)) => {
                        stub_provider = Some(p);
                        BackendKind::OpenAI
                    }
                    Some(("anthropic", _)) => BackendKind::Anthropic,
                    Some(("gemini", _)) => BackendKind::Gemini,
                    Some(("vertex", _)) => BackendKind::Vertex,
                    Some(("azure", _)) => BackendKind::AzureOpenAI,
                    Some(("bedrock", _)) => BackendKind::Bedrock,
                    _ => {
                        tracing::warn!(provider = %other, "provider found in registry but protocol not mappable, treating as openai-compatible");
                        stub_provider = Some(p);
                        BackendKind::OpenAI
                    }
                };
                resolved
            } else {
                tracing::warn!(
                    provider = %other,
                    "unknown LiteLLM provider, treating as openai-compatible"
                );
                BackendKind::OpenAI
            }
        }
    };
    (kind, model_name.to_string(), stub_provider)
}

// ---- Backend deduplication key ----

/// Unique identity for a backend: same kind + base_url + api_key share one connection pool.
#[derive(Hash, PartialEq, Eq, Clone)]
struct BackendKey {
    kind: String,
    base_url: String,
    /// Hash of the API key (not the key itself) to avoid holding secrets in hash keys.
    api_key_hash: u64,
}

fn hash_string(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ---- Conversion ----

/// Parse a LiteLLM config.yaml string and produce a MultiConfig + ModelRouter.
///
/// # Panics
/// On invalid YAML, missing required fields, or unresolvable env var references.
/// Parsed result from a LiteLLM config file.
pub struct LiteLLMParsed {
    pub multi_config: MultiConfig,
    pub router: ModelRouter,
    /// Webhook callback URLs from litellm_settings.callbacks (non-named entries).
    pub callback_urls: Vec<String>,
    /// True when "langfuse" appears in litellm_settings.callbacks.
    pub langfuse_requested: bool,
    /// Resolved `general_settings.master_key`, if present.
    /// Caller should apply as PROXY_API_KEYS if that var is not already set.
    pub master_key: Option<String>,
}

/// Parse a LiteLLM YAML config and return the multi-backend config + model router pair.
pub fn from_litellm_yaml(yaml: &str) -> (MultiConfig, ModelRouter) {
    let parsed = parse_litellm_yaml(yaml);
    (parsed.multi_config, parsed.router)
}

/// Parse a LiteLLM YAML config into the intermediate `LiteLLMParsed` struct.
/// Panics on invalid YAML (startup-time validation; misconfiguration is unrecoverable).
pub fn parse_litellm_yaml(yaml: &str) -> LiteLLMParsed {
    let config: LiteLLMConfig =
        serde_yaml::from_str(yaml).unwrap_or_else(|e| panic!("invalid LiteLLM config YAML: {e}"));

    if config.model_list.is_empty() {
        panic!("LiteLLM config must define at least one entry in model_list");
    }

    // Resolve general_settings.master_key but do not call set_var here.
    // The caller applies it in the consolidated env override block.
    let master_key = if let Some(ref gs) = config.general_settings {
        let mk = gs.master_key.as_ref().map(|mk| {
            resolve_env_value(mk).unwrap_or_else(|e| panic!("general_settings.master_key: {e}"))
        });
        // Log unsupported keys at warn.
        for key in gs._extra.keys() {
            tracing::warn!(key = %key, "unsupported general_settings key (ignored)");
        }
        mk
    } else {
        None
    };

    if let Some(ref ls) = config.litellm_settings {
        for key in ls._extra.keys() {
            tracing::warn!(key = %key, "unsupported litellm_settings key (ignored)");
        }
    }

    if let Some(ref rs) = config.router_settings {
        for key in rs._extra.keys() {
            tracing::warn!(key = %key, "unsupported router_settings key (ignored)");
        }
    }

    let listen_port = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);

    let log_bodies = std::env::var("LOG_BODIES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let redact_secrets = std::env::var("REDACT_SECRETS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let tls = TlsConfig::from_env();

    // Group model_list entries into deduplicated backends + deployment list.
    let mut backend_map: HashMap<BackendKey, (String, BackendConfig)> = HashMap::new();
    let mut backend_counter = 0u32;
    // model_name -> Vec<(backend_name, actual_model, rpm, tpm)>
    let mut model_deployments: HashMap<String, Vec<DeploymentSpec>> = HashMap::new();

    for entry in &config.model_list {
        let (kind, actual_model, stub_provider) = parse_provider_model(&entry.litellm_params.model);
        let params = &entry.litellm_params;

        let api_key = super::sanitize_api_key(
            &params
                .api_key
                .as_deref()
                .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("model_list api_key: {e}")))
                .unwrap_or_else(|| {
                    // Fall back to the provider's own env vars when no api_key in YAML.
                    stub_provider
                        .and_then(|p| p.env_vars.iter().find_map(|v| std::env::var(v).ok()))
                        .unwrap_or_default()
                }),
        );

        let base_url = resolve_base_url(&kind, params, stub_provider);

        let bk = BackendKey {
            kind: format!("{kind:?}"),
            base_url: base_url.clone(),
            api_key_hash: hash_string(&api_key),
        };

        let backend_name = if let Some((name, _)) = backend_map.get(&bk) {
            name.clone()
        } else {
            let name = format!("litellm_{backend_counter}");
            backend_counter += 1;

            let bc = build_backend_config(
                &name, &kind, &api_key, &base_url, params, &tls, log_bodies, &config,
            );
            backend_map.insert(bk, (name.clone(), bc));
            name
        };

        model_deployments
            .entry(entry.model_name.clone())
            .or_default()
            .push(DeploymentSpec {
                backend_name,
                actual_model,
                rpm: params.rpm,
                tpm: params.tpm,
                weight: params.weight,
            });
    }

    // Build MultiConfig backends (ordered).
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
        expose_degradation_warnings: false, // overridden in MultiConfig::load()
    };

    // Determine routing strategy from router_settings.
    let strategy = config
        .router_settings
        .as_ref()
        .and_then(|rs| rs.routing_strategy.as_deref())
        .map(parse_routing_strategy_str)
        .unwrap_or_default();

    if strategy != RoutingStrategy::RoundRobin {
        tracing::info!(strategy = ?strategy, "using routing strategy from config");
    }

    // Build ModelRouter.
    let mut routes: HashMap<String, Vec<Arc<Deployment>>> = HashMap::new();
    for (model_name, specs) in model_deployments {
        let deployments = specs
            .into_iter()
            .map(|s| {
                Arc::new(Deployment::with_weight(
                    s.backend_name,
                    s.actual_model,
                    s.rpm,
                    s.tpm,
                    s.weight.unwrap_or(1),
                ))
            })
            .collect();
        routes.insert(model_name, deployments);
    }

    let router = ModelRouter::with_strategy(routes, strategy);

    let callbacks = config
        .litellm_settings
        .as_ref()
        .map(|s| s.callbacks.clone())
        .unwrap_or_default();
    let langfuse_requested = callbacks.iter().any(|c| c.eq_ignore_ascii_case("langfuse"));
    let callback_urls: Vec<String> = callbacks
        .into_iter()
        .filter(|c| !c.eq_ignore_ascii_case("langfuse"))
        .collect();

    LiteLLMParsed {
        multi_config: multi,
        router,
        callback_urls,
        langfuse_requested,
        master_key,
    }
}

struct DeploymentSpec {
    backend_name: String,
    actual_model: String,
    rpm: Option<u32>,
    tpm: Option<u64>,
    weight: Option<u32>,
}

/// Extract `general_settings.master_key` from a LiteLLM YAML string without
/// performing full config parsing. Used by the synchronous `fn main()` to apply
/// the key via `set_var` before the tokio runtime spawns worker threads.
pub fn extract_master_key(yaml: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Probe {
        general_settings: Option<GeneralSettings>,
    }
    let probe: Probe = serde_yaml::from_str(yaml).ok()?;
    let gs = probe.general_settings?;
    let raw = gs.master_key?;
    resolve_env_value(&raw).ok()
}

/// Determine the base URL for a deployment, applying provider-specific defaults.
fn resolve_base_url(
    kind: &BackendKind,
    params: &LiteLLMParams,
    stub_provider: Option<&'static anyllm_providers::ProviderDef>,
) -> String {
    if let Some(ref url) = params.api_base {
        let resolved =
            resolve_env_value(url).unwrap_or_else(|e| panic!("model_list api_base: {e}"));
        return resolved;
    }
    match kind {
        BackendKind::OpenAI => {
            // Use the stub provider's default URL when available (e.g. groq, xai, mistral).
            // If a known provider has no safe global default, require explicit api_base.
            let url = if let Some(provider) = stub_provider {
                if provider.default_base_url.is_empty() {
                    panic!(
                        "model_list provider '{}' requires api_base because it has no safe global API base URL",
                        provider.id
                    );
                }
                provider.default_base_url
            } else {
                "https://api.openai.com"
            };
            super::strip_v1_suffix(url).to_string()
        }
        BackendKind::Gemini => {
            "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
        }
        BackendKind::Anthropic => std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
        BackendKind::Bedrock => {
            // For Bedrock, base_url stores the region.
            params
                .aws_region_name
                .as_deref()
                .map(|v| v.to_string())
                .or_else(|| std::env::var("AWS_REGION").ok())
                .unwrap_or_else(|| "us-east-1".to_string())
        }
        // Azure and Vertex require api_base in the config.
        BackendKind::AzureOpenAI => {
            panic!("api_base is required for azure deployments in model_list")
        }
        BackendKind::Vertex => {
            panic!("api_base is required for vertex deployments in model_list")
        }
    }
}

/// Build a BackendConfig from LiteLLM model_list params.
#[allow(clippy::too_many_arguments)]
fn build_backend_config(
    name: &str,
    kind: &BackendKind,
    api_key: &str,
    base_url: &str,
    params: &LiteLLMParams,
    tls: &TlsConfig,
    log_bodies: bool,
    config: &LiteLLMConfig,
) -> BackendConfig {
    let backend_auth = match kind {
        BackendKind::AzureOpenAI => BackendAuth::AzureApiKey(api_key.to_string()),
        BackendKind::Gemini | BackendKind::Vertex => BackendAuth::GoogleApiKey(api_key.to_string()),
        _ => BackendAuth::BearerToken(api_key.to_string()),
    };

    // For Azure, build deployment URL from api_base.
    let effective_url = if *kind == BackendKind::AzureOpenAI {
        let api_version = params.api_version.as_deref().unwrap_or("2024-10-21");
        // LiteLLM api_base for Azure is the resource endpoint.
        // We need to append the deployment path.
        if base_url.contains("/openai/deployments/") {
            // Already a full deployment URL.
            base_url.to_string()
        } else {
            format!(
                "{}/openai/deployments/chat/completions?api-version={api_version}",
                base_url.trim_end_matches('/')
            )
        }
    } else {
        // Validate non-Azure URLs.
        if *kind != BackendKind::Bedrock {
            if let Err(e) = validate_base_url(base_url) {
                panic!("backend '{name}' base_url rejected: {e}");
            }
        }
        base_url.to_string()
    };

    // Bedrock credentials.
    let bedrock_credentials = if *kind == BackendKind::Bedrock {
        let region = params
            .aws_region_name
            .as_deref()
            .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_string());

        let access_key = params
            .aws_access_key_id
            .as_deref()
            .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .unwrap_or_else(|| panic!("backend '{name}': aws_access_key_id required for bedrock"));

        let secret_key = params
            .aws_secret_access_key
            .as_deref()
            .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("backend '{name}': {e}")))
            .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
            .unwrap_or_else(|| {
                panic!("backend '{name}': aws_secret_access_key required for bedrock")
            });

        // Store region as base_url for Bedrock (matches existing convention).
        // The effective_url was already set to the region string.
        let _ = region; // region is used as base_url via resolve_base_url

        Some(aws_credential_types::Credentials::new(
            access_key,
            secret_key,
            None, // session token not commonly in LiteLLM configs
            None,
            "litellm-config",
        ))
    } else {
        None
    };

    // Placeholder model mapping: with model router, these are not used for routing.
    // They serve as fallback for Anthropic model name translation if needed.
    let model_mapping = ModelMapping {
        big_model: String::new(),
        small_model: String::new(),
    };

    let _num_retries = config.litellm_settings.as_ref().and_then(|s| s.num_retries);
    let _request_timeout = config
        .litellm_settings
        .as_ref()
        .and_then(|s| s.request_timeout);

    BackendConfig {
        kind: kind.clone(),
        api_key: api_key.to_string(),
        base_url: effective_url,
        api_format: OpenAIApiFormat::Chat,
        model_mapping,
        tls: tls.clone(),
        backend_auth,
        log_bodies,
        omit_stream_options: false,
        stream_timeout_secs: std::env::var("REQUEST_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900u64),
        bedrock_credentials,
    }
}

#[cfg(test)]
mod tests;
