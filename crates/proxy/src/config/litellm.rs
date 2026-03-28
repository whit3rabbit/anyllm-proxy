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
fn parse_routing_strategy(s: &str) -> RoutingStrategy {
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
fn parse_provider_model(model: &str) -> (BackendKind, String) {
    let (provider, model_name) = model.split_once('/').unwrap_or(("openai", model));
    let kind = match provider.to_ascii_lowercase().as_str() {
        "openai" => BackendKind::OpenAI,
        "azure" => BackendKind::AzureOpenAI,
        "vertex_ai" | "vertex" => BackendKind::Vertex,
        "gemini" => BackendKind::Gemini,
        "anthropic" => BackendKind::Anthropic,
        "bedrock" => BackendKind::Bedrock,
        other => {
            tracing::warn!(
                provider = %other,
                "unknown LiteLLM provider, treating as openai-compatible"
            );
            BackendKind::OpenAI
        }
    };
    (kind, model_name.to_string())
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
    /// Webhook callback URLs from litellm_settings.callbacks.
    pub callback_urls: Vec<String>,
    /// Resolved `general_settings.master_key`, if present.
    /// Caller should apply as PROXY_API_KEYS if that var is not already set.
    pub master_key: Option<String>,
}

pub fn from_litellm_yaml(yaml: &str) -> (MultiConfig, ModelRouter) {
    let parsed = parse_litellm_yaml(yaml);
    (parsed.multi_config, parsed.router)
}

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

    let tls = TlsConfig::from_env();

    // Group model_list entries into deduplicated backends + deployment list.
    let mut backend_map: HashMap<BackendKey, (String, BackendConfig)> = HashMap::new();
    let mut backend_counter = 0u32;
    // model_name -> Vec<(backend_name, actual_model, rpm, tpm)>
    let mut model_deployments: HashMap<String, Vec<DeploymentSpec>> = HashMap::new();

    for entry in &config.model_list {
        let (kind, actual_model) = parse_provider_model(&entry.litellm_params.model);
        let params = &entry.litellm_params;

        let api_key = params
            .api_key
            .as_deref()
            .map(|v| resolve_env_value(v).unwrap_or_else(|e| panic!("model_list api_key: {e}")))
            .unwrap_or_default();

        let base_url = resolve_base_url(&kind, params);

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
        default_backend,
        backends,
    };

    // Determine routing strategy from router_settings.
    let strategy = config
        .router_settings
        .as_ref()
        .and_then(|rs| rs.routing_strategy.as_deref())
        .map(parse_routing_strategy)
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

    let callback_urls = config
        .litellm_settings
        .as_ref()
        .map(|s| s.callbacks.clone())
        .unwrap_or_default();

    LiteLLMParsed {
        multi_config: multi,
        router,
        callback_urls,
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

/// Determine the base URL for a deployment, applying provider-specific defaults.
fn resolve_base_url(kind: &BackendKind, params: &LiteLLMParams) -> String {
    if let Some(ref url) = params.api_base {
        let resolved =
            resolve_env_value(url).unwrap_or_else(|e| panic!("model_list api_base: {e}"));
        return resolved;
    }
    match kind {
        BackendKind::OpenAI => "https://api.openai.com".to_string(),
        BackendKind::Gemini => {
            "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
        }
        BackendKind::Anthropic => "https://api.anthropic.com".to_string(),
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
        bedrock_credentials,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_model_openai() {
        let (kind, model) = parse_provider_model("openai/gpt-4o");
        assert_eq!(kind, BackendKind::OpenAI);
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn parse_provider_model_azure() {
        let (kind, model) = parse_provider_model("azure/gpt-4o-eu");
        assert_eq!(kind, BackendKind::AzureOpenAI);
        assert_eq!(model, "gpt-4o-eu");
    }

    #[test]
    fn parse_provider_model_no_prefix() {
        let (kind, model) = parse_provider_model("gpt-4o");
        assert_eq!(kind, BackendKind::OpenAI);
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn parse_provider_model_vertex_ai() {
        let (kind, model) = parse_provider_model("vertex_ai/gemini-pro");
        assert_eq!(kind, BackendKind::Vertex);
        assert_eq!(model, "gemini-pro");
    }

    #[test]
    fn parse_provider_model_bedrock() {
        let (kind, model) = parse_provider_model("bedrock/anthropic.claude-v2");
        assert_eq!(kind, BackendKind::Bedrock);
        assert_eq!(model, "anthropic.claude-v2");
    }

    #[test]
    fn parse_provider_model_unknown_treated_as_openai() {
        let (kind, model) = parse_provider_model("groq/llama-70b");
        assert_eq!(kind, BackendKind::OpenAI);
        assert_eq!(model, "llama-70b");
    }

    #[test]
    fn minimal_litellm_config() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test-key
"#;

        let (multi, router) = from_litellm_yaml(yaml);
        assert_eq!(multi.backends.len(), 1);
        assert!(router.has_model("gpt-4o"));

        let routed = router.route("gpt-4o").unwrap();
        assert_eq!(routed.actual_model, "gpt-4o");
    }

    #[test]
    fn multiple_deployments_same_model() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-1
      rpm: 100
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-2
      rpm: 200
"#;

        let (multi, router) = from_litellm_yaml(yaml);
        // Different api_keys = different backends
        assert_eq!(multi.backends.len(), 2);
        assert!(router.has_model("gpt-4o"));

        // Should round-robin between the two
        let r0 = router.route("gpt-4o").unwrap();
        let r1 = router.route("gpt-4o").unwrap();
        assert_ne!(r0.backend_name, r1.backend_name);
    }

    #[test]
    fn backend_deduplication() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-same-key
  - model_name: gpt-4o-mini
    litellm_params:
      model: openai/gpt-4o-mini
      api_key: sk-same-key
"#;

        let (multi, router) = from_litellm_yaml(yaml);
        // Same provider + base_url + api_key = one backend
        assert_eq!(multi.backends.len(), 1);
        assert!(router.has_model("gpt-4o"));
        assert!(router.has_model("gpt-4o-mini"));
    }

    #[test]
    fn os_environ_syntax_in_litellm_yaml() {
        // Set env var for test
        unsafe { std::env::set_var("TEST_LITELLM_KEY", "sk-from-env") };

        let yaml = r#"
model_list:
  - model_name: test-model
    litellm_params:
      model: openai/gpt-4o
      api_key: "os.environ/TEST_LITELLM_KEY"
"#;

        let (multi, _) = from_litellm_yaml(yaml);
        let bc = multi.backends.values().next().unwrap();
        assert_eq!(bc.api_key, "sk-from-env");

        unsafe { std::env::remove_var("TEST_LITELLM_KEY") };
    }

    #[test]
    fn unknown_settings_are_accepted() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
      some_unknown_param: true

litellm_settings:
  drop_params: true
  some_future_setting: 42

general_settings:
  some_unknown_general: "value"
"#;

        // Should not panic; unknown fields are captured by serde(flatten).
        let (multi, router) = from_litellm_yaml(yaml);
        assert_eq!(multi.backends.len(), 1);
        assert!(router.has_model("gpt-4o"));
    }

    #[test]
    fn routing_strategy_parsed() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: least-busy
"#;
        let (_, router) = from_litellm_yaml(yaml);
        assert_eq!(router.strategy(), RoutingStrategy::LeastBusy);
    }

    #[test]
    fn routing_strategy_latency() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: latency-based-routing
"#;
        let (_, router) = from_litellm_yaml(yaml);
        assert_eq!(router.strategy(), RoutingStrategy::LatencyBased);
    }

    #[test]
    fn routing_strategy_cost_based() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: cost-based
"#;
        let (_, router) = from_litellm_yaml(yaml);
        assert_eq!(router.strategy(), RoutingStrategy::CostBased);
    }

    #[test]
    fn routing_strategy_defaults_to_round_robin() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
"#;
        let (_, router) = from_litellm_yaml(yaml);
        assert_eq!(router.strategy(), RoutingStrategy::RoundRobin);
    }

    #[test]
    fn weight_field_parsed() {
        let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-1
      weight: 3
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-2
      weight: 1

router_settings:
  routing_strategy: weighted
"#;
        let (_, router) = from_litellm_yaml(yaml);
        assert_eq!(router.strategy(), RoutingStrategy::Weighted);
        assert!(router.has_model("gpt-4o"));
    }

    #[test]
    #[should_panic(expected = "at least one entry")]
    fn empty_model_list_panics() {
        let yaml = r#"
model_list: []
"#;
        from_litellm_yaml(yaml);
    }

    #[test]
    fn gemini_provider() {
        let yaml = r#"
model_list:
  - model_name: gemini-pro
    litellm_params:
      model: gemini/gemini-pro
      api_key: AIzaSy-test
"#;

        let (multi, router) = from_litellm_yaml(yaml);
        assert_eq!(multi.backends.len(), 1);
        let bc = multi.backends.values().next().unwrap();
        assert_eq!(bc.kind, BackendKind::Gemini);
        assert!(router.has_model("gemini-pro"));
    }
}
