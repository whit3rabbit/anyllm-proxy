# Model Routing UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a simple native YAML config format (`models:` key) that makes routing ergonomic without requiring LiteLLM's verbose `litellm_params:` nesting, while reusing all existing `ModelRouter` strategies (already implemented).

**Architecture:** Detect `models:` at YAML root -> dispatch to new `config/simple.rs` parser; detect `model_list:` -> existing LiteLLM parser. Both paths produce the same `MultiConfig + ModelRouter`. Provider API keys default to standard env vars (e.g., `OPENAI_API_KEY`) so entries stay terse.

**Tech Stack:** Rust stable 1.83+, `serde_yaml`, `serde` untagged enum for string/struct model entries. No new crate dependencies.

---

## Current State (read before writing any code)

- `crates/proxy/src/config/model_router.rs`: `ModelRouter`, `Deployment`, `RoutingStrategy` — all five strategies already implemented and tested.
- `crates/proxy/src/config/litellm.rs`: LiteLLM YAML parser (`model_list:` + `litellm_params:` nesting). Produces `LiteLLMParsed { multi_config, router, ... }`.
- `crates/proxy/src/config/mod.rs:492`: `MultiConfig::load()` dispatches `.yaml`/`.yml` -> `litellm::parse_litellm_yaml`, other extensions -> TOML, no file -> env vars.
- `crates/proxy/src/config/mod.rs`: `BackendKind`, `BackendConfig`, `BackendAuth`, `MultiConfig`, `ModelMapping`, `TlsConfig`, `validate_base_url` — all already defined; reuse them.

---

## New Config Format (simple)

```yaml
# anyllm.yaml
routing_strategy: latency-based   # round-robin | least-busy | latency-based | weighted | cost-based
listen_port: 3000                  # optional, default: 3000
log_bodies: false                  # optional, default: false

models:
  # String shorthand: "model" (openai default) or "provider/model"
  - gpt-4o
  - openai/gpt-4o-mini
  - anthropic/claude-3-5-sonnet-20241022

  # Full form with all options
  - name: smart               # virtual model name clients send (defaults to model if omitted)
    model: gpt-4o             # actual model name sent to backend
    provider: openai          # openai | azure | vertex | gemini | anthropic | bedrock
    weight: 3                 # for weighted routing (default: 1)
    rpm: 1000                 # optional per-deployment rate limit
    tpm: 500000               # optional per-deployment rate limit
    api_base: https://...     # optional, overrides provider default
    api_key: sk-...           # optional, overrides provider env var

  # Same virtual name, second deployment = round-robin/failover within "smart"
  - name: smart
    model: claude-3-5-sonnet-20241022
    provider: anthropic
    weight: 1
```

Provider-to-env-var defaults (applied when `api_key` is omitted):

| provider    | env var                        |
|-------------|-------------------------------|
| openai      | `OPENAI_API_KEY`              |
| anthropic   | `ANTHROPIC_API_KEY`           |
| gemini      | `GEMINI_API_KEY`              |
| vertex      | `VERTEX_API_KEY` or `GOOGLE_ACCESS_TOKEN` |
| azure       | `AZURE_OPENAI_API_KEY`        |
| bedrock     | `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` |

Detection in `MultiConfig::load()`: parse YAML as `serde_yaml::Value`, check for `models` key at root. If present: simple format. If `model_list` key: LiteLLM format. Fail with a clear message if neither.

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `crates/proxy/src/config/simple.rs` | `SimpleConfig` serde types, `parse_simple_yaml()`, `SimpleConfigError` |
| Modify | `crates/proxy/src/config/mod.rs:1-10` | Add `pub mod simple;` |
| Modify | `crates/proxy/src/config/mod.rs:492-537` | Add `models:` detection branch before LiteLLM branch |

---

## Task 1: `SimpleConfig` serde types (no logic yet)

**Files:**
- Create: `crates/proxy/src/config/simple.rs`

- [ ] **Step 1: Write the failing test — types compile**

```rust
// At the bottom of crates/proxy/src/config/simple.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_config_roundtrip_string_shorthand() {
        let yaml = r#"
models:
  - gpt-4o
  - openai/gpt-4o-mini
  - anthropic/claude-3-5-sonnet-20241022
"#;
        let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.models.len(), 3);
        // All three are string shorthand
        match &cfg.models[0] {
            SimpleModelEntry::Shorthand(s) => assert_eq!(s, "gpt-4o"),
            SimpleModelEntry::Full(_) => panic!("expected shorthand"),
        }
    }

    #[test]
    fn simple_config_roundtrip_full_entry() {
        let yaml = r#"
routing_strategy: weighted
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
    rpm: 1000
"#;
        let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.routing_strategy.as_deref(), Some("weighted"));
        assert_eq!(cfg.models.len(), 1);
        match &cfg.models[0] {
            SimpleModelEntry::Full(f) => {
                assert_eq!(f.name.as_deref(), Some("smart"));
                assert_eq!(f.model, "gpt-4o");
                assert_eq!(f.provider.as_deref(), Some("openai"));
                assert_eq!(f.weight, Some(3));
                assert_eq!(f.rpm, Some(1000));
            }
            SimpleModelEntry::Shorthand(_) => panic!("expected full entry"),
        }
    }

    #[test]
    fn simple_config_mixed_entries() {
        let yaml = r#"
models:
  - gpt-4o
  - name: my-model
    model: claude-3-5-sonnet-20241022
    provider: anthropic
"#;
        let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.models.len(), 2);
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail (file doesn't exist yet)**

```bash
cargo test -p anyllm_proxy simple_config_roundtrip 2>&1 | head -20
```

Expected: `error[E0433]: failed to resolve: use of undeclared crate or module 'simple'`

- [ ] **Step 3: Write the serde types**

Create `crates/proxy/src/config/simple.rs` with this content:

```rust
//! Simple native YAML config format for anyllm-proxy.
//!
//! Activated when the config file contains a top-level `models:` key
//! (as opposed to LiteLLM's `model_list:`).
//!
//! See docs/superpowers/plans/2026-03-30-model-routing-ux.md for format spec.
use serde::Deserialize;

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
    /// List of model deployments.
    #[serde(default)]
    pub models: Vec<SimpleModelEntry>,
}

/// A model entry: either a string shorthand or a full struct.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SimpleModelEntry {
    /// String shorthand: "model-name" or "provider/model-name".
    Shorthand(String),
    /// Full form with all fields.
    Full(SimpleModelFull),
}

/// Full model entry with all optional fields.
#[derive(Debug, Deserialize)]
pub struct SimpleModelFull {
    /// Virtual model name clients send in requests.
    /// Defaults to `model` if omitted.
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
```

Add `pub mod simple;` to `crates/proxy/src/config/mod.rs` (after `pub mod litellm;`):

```rust
pub mod env_aliases;
pub mod litellm;
pub mod model_router;
pub mod simple;        // <-- add this line
mod tls;
mod url_validation;
```

- [ ] **Step 4: Run the serde tests**

```bash
cargo test -p anyllm_proxy simple_config_roundtrip 2>&1
cargo test -p anyllm_proxy simple_config_mixed 2>&1
```

Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/config/simple.rs crates/proxy/src/config/mod.rs
git commit -m "feat(config): add SimpleConfig serde types for simple YAML format"
```

---

## Task 2: `parse_simple_yaml()` — conversion to `MultiConfig + ModelRouter`

**Files:**
- Modify: `crates/proxy/src/config/simple.rs` (add parsing logic below the types)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `simple.rs`:

```rust
    #[test]
    fn parse_single_openai_model() {
        // Uses OPENAI_API_KEY from env (set to empty string for test)
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
        let yaml = r#"
models:
  - gpt-4o
"#;
        let parsed = parse_simple_yaml(yaml);
        assert_eq!(parsed.multi_config.backends.len(), 1);
        assert!(parsed.router.has_model("gpt-4o"));
        let routed = parsed.router.route("gpt-4o").unwrap();
        assert_eq!(routed.actual_model, "gpt-4o");
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn parse_provider_slash_model_shorthand() {
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "sk-openai");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-anthropic");
        };
        let yaml = r#"
models:
  - openai/gpt-4o
  - anthropic/claude-3-5-sonnet-20241022
"#;
        let parsed = parse_simple_yaml(yaml);
        assert_eq!(parsed.multi_config.backends.len(), 2);
        assert!(parsed.router.has_model("gpt-4o"));
        assert!(parsed.router.has_model("claude-3-5-sonnet-20241022"));
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("ANTHROPIC_API_KEY");
        };
    }

    #[test]
    fn parse_full_entry_with_virtual_name() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
        let yaml = r#"
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
"#;
        let parsed = parse_simple_yaml(yaml);
        // Virtual name is "smart", not "gpt-4o"
        assert!(parsed.router.has_model("smart"));
        assert!(!parsed.router.has_model("gpt-4o"));
        let routed = parsed.router.route("smart").unwrap();
        assert_eq!(routed.actual_model, "gpt-4o");
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn parse_routing_strategy_latency() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
        let yaml = r#"
routing_strategy: latency-based
models:
  - gpt-4o
"#;
        let parsed = parse_simple_yaml(yaml);
        assert_eq!(
            parsed.router.strategy(),
            crate::config::model_router::RoutingStrategy::LatencyBased
        );
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn parse_weighted_two_deployments_same_virtual_name() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
        let yaml = r#"
routing_strategy: weighted
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
  - name: smart
    model: gpt-4o-mini
    provider: openai
    weight: 1
"#;
        let parsed = parse_simple_yaml(yaml);
        // Both entries map to virtual name "smart"
        assert!(parsed.router.has_model("smart"));
        // list_models: "smart" has 2 deployments
        let list = parsed.router.list_models();
        let (_, count) = list.iter().find(|(n, _)| *n == "smart").unwrap();
        assert_eq!(*count, 2);
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn parse_api_key_inline_overrides_env() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-from-env") };
        let yaml = r#"
models:
  - name: my-model
    model: gpt-4o
    provider: openai
    api_key: sk-inline-key
"#;
        let parsed = parse_simple_yaml(yaml);
        let bc = parsed.multi_config.backends.values().next().unwrap();
        assert_eq!(bc.api_key, "sk-inline-key");
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    #[should_panic(expected = "must define at least one model")]
    fn parse_empty_models_panics() {
        let yaml = "models: []\n";
        parse_simple_yaml(yaml);
    }
```

- [ ] **Step 2: Run to confirm failures**

```bash
cargo test -p anyllm_proxy parse_single_openai_model 2>&1 | head -20
```

Expected: `error[E0425]: cannot find function 'parse_simple_yaml'`

- [ ] **Step 3: Write `parse_simple_yaml()` and helpers**

Add to `crates/proxy/src/config/simple.rs`, after the type definitions:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use indexmap::IndexMap;

use super::model_router::{Deployment, ModelRouter, RoutingStrategy};
use super::{
    validate_base_url, BackendAuth, BackendConfig, BackendKind, ModelMapping,
    MultiConfig, OpenAIApiFormat, TlsConfig,
};
use super::litellm::parse_routing_strategy_str;

/// Parsed result from a simple config file.
pub struct SimpleParsed {
    pub multi_config: MultiConfig,
    pub router: ModelRouter,
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

    let log_bodies = config
        .log_bodies
        .unwrap_or_else(|| {
            std::env::var("LOG_BODIES")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });

    let tls = TlsConfig::from_env();

    // Dedup key: provider kind + base_url + api_key hash.
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

    let mut backend_map: HashMap<BackendKey, (String, BackendConfig)> = HashMap::new();
    let mut backend_counter = 0u32;
    // virtual_name -> Vec<(backend_name, actual_model, rpm, tpm, weight)>
    struct DepSpec {
        backend_name: String,
        actual_model: String,
        rpm: Option<u32>,
        tpm: Option<u64>,
        weight: u32,
    }
    let mut model_deployments: HashMap<String, Vec<DepSpec>> = HashMap::new();

    for entry in &config.models {
        // Normalize to (virtual_name, provider, actual_model, full_options).
        let full: NormalizedEntry = normalize_entry(entry);

        let kind = parse_kind(&full.provider);
        let api_key = full
            .api_key
            .clone()
            .unwrap_or_else(|| default_api_key_for_provider(&full.provider, &kind));
        let base_url = full
            .api_base
            .clone()
            .unwrap_or_else(|| default_base_url(&kind, &full));

        if kind != BackendKind::Bedrock {
            if let Err(e) = validate_base_url(&base_url) {
                panic!("model '{}' base_url rejected: {e}", full.virtual_name);
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
            let bc = build_backend_config(&name, &kind, &api_key, &base_url, &full, &tls, log_bodies);
            backend_map.insert(bk, (name.clone(), bc));
            name
        };

        model_deployments
            .entry(full.virtual_name.clone())
            .or_default()
            .push(DepSpec {
                backend_name,
                actual_model: full.actual_model.clone(),
                rpm: full.rpm,
                tpm: full.tpm,
                weight: full.weight.unwrap_or(1),
            });
    }

    // Build MultiConfig.
    let mut backends = IndexMap::new();
    for (name, bc) in backend_map.values() {
        backends.insert(name.clone(), bc.clone());
    }
    let default_backend = backends.keys().next().cloned().expect("at least one backend");
    let multi = MultiConfig { listen_port, log_bodies, default_backend, backends };

    // Routing strategy.
    let strategy = config
        .routing_strategy
        .as_deref()
        .map(parse_routing_strategy_str)
        .unwrap_or_default();

    // Build ModelRouter.
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

    SimpleParsed { multi_config: multi, router }
}

// ---- Internal helpers ----

struct NormalizedEntry {
    virtual_name: String,
    provider: String,
    actual_model: String,
    api_key: Option<String>,
    api_base: Option<String>,
    weight: Option<u32>,
    rpm: Option<u32>,
    tpm: Option<u64>,
    // Azure
    deployment: Option<String>,
    api_version: Option<String>,
    // Vertex
    project: Option<String>,
    region: Option<String>,
    // Bedrock
    aws_region: Option<String>,
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
}

/// Turn a `SimpleModelEntry` into a normalized form.
fn normalize_entry(entry: &SimpleModelEntry) -> NormalizedEntry {
    match entry {
        SimpleModelEntry::Shorthand(s) => {
            // "model-name" or "provider/model-name"
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
        BackendKind::Bedrock => return String::new(), // SigV4, not a bearer token
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
        BackendKind::OpenAI => std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string()),
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
                .or_else(|| None) // caller will panic if missing
                .unwrap_or_else(|| {
                    Box::leak(
                        std::env::var("VERTEX_PROJECT")
                            .expect("project is required for vertex provider")
                            .into_boxed_str(),
                    )
                });
            let region = entry
                .region
                .as_deref()
                .unwrap_or_else(|| {
                    Box::leak(
                        std::env::var("VERTEX_REGION")
                            .expect("region is required for vertex provider")
                            .into_boxed_str(),
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
                .unwrap_or_else(|| {
                    Box::leak(
                        std::env::var("AZURE_OPENAI_ENDPOINT")
                            .expect("api_base (or AZURE_OPENAI_ENDPOINT) is required for azure provider")
                            .into_boxed_str(),
                    )
                });
            let deployment = entry
                .deployment
                .as_deref()
                .or_else(|| Some("chat"))
                .unwrap_or("chat");
            let version = entry
                .api_version
                .as_deref()
                .unwrap_or("2024-10-21");
            format!(
                "{}/openai/deployments/{deployment}/chat/completions?api-version={version}",
                endpoint.trim_end_matches('/')
            )
        }
        BackendKind::Bedrock => {
            entry
                .aws_region
                .as_deref()
                .unwrap_or_else(|| {
                    Box::leak(
                        std::env::var("AWS_REGION")
                            .unwrap_or_else(|_| "us-east-1".to_string())
                            .into_boxed_str(),
                    )
                })
                .to_string()
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
        let region = entry
            .aws_region
            .as_deref()
            .map(|s| s.to_string())
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_string());

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
            .unwrap_or_else(|| panic!("backend '{name}': aws_secret_access_key required for bedrock"));

        let _ = region;
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
        model_mapping: ModelMapping { big_model: String::new(), small_model: String::new() },
        tls: tls.clone(),
        backend_auth,
        log_bodies,
        omit_stream_options: false,
        bedrock_credentials,
    }
}
```

- [ ] **Step 4: Expose `parse_routing_strategy_str` from `litellm.rs`**

In `crates/proxy/src/config/litellm.rs`, change line `fn parse_routing_strategy` to `pub(crate) fn parse_routing_strategy_str`:

Find:
```rust
fn parse_routing_strategy(s: &str) -> RoutingStrategy {
```

Replace with:
```rust
pub(crate) fn parse_routing_strategy_str(s: &str) -> RoutingStrategy {
```

Also update the single call-site within `litellm.rs` from `parse_routing_strategy` to `parse_routing_strategy_str`.

- [ ] **Step 5: Run all new tests**

```bash
cargo test -p anyllm_proxy parse_single_openai 2>&1
cargo test -p anyllm_proxy parse_provider_slash 2>&1
cargo test -p anyllm_proxy parse_full_entry_with_virtual 2>&1
cargo test -p anyllm_proxy parse_routing_strategy_latency 2>&1
cargo test -p anyllm_proxy parse_weighted_two 2>&1
cargo test -p anyllm_proxy parse_api_key_inline 2>&1
cargo test -p anyllm_proxy parse_empty_models 2>&1
```

Expected: all 7 tests pass.

- [ ] **Step 6: Run full test suite to check for regressions**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -10
```

Expected: output ends with `test result: ok. N passed; 0 failed`.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/config/simple.rs crates/proxy/src/config/litellm.rs
git commit -m "feat(config): implement parse_simple_yaml with provider env-var defaults and routing strategies"
```

---

## Task 3: Wire simple format into `MultiConfig::load()`

**Files:**
- Modify: `crates/proxy/src/config/mod.rs:492-537`

- [ ] **Step 1: Write the failing integration test**

Add to `crates/proxy/src/config/mod.rs` tests (or a new integration test file at `crates/proxy/tests/simple_config.rs`):

```rust
// crates/proxy/tests/simple_config.rs
//! Integration: MultiConfig::load() detects and dispatches simple format.

use anyllm_proxy::config::MultiConfig;

#[test]
fn load_dispatches_simple_format_by_models_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routing.yaml");
    std::fs::write(
        &path,
        r#"
models:
  - openai/gpt-4o
  - openai/gpt-4o-mini
routing_strategy: least-busy
"#,
    )
    .unwrap();

    unsafe { std::env::set_var("PROXY_CONFIG", path.to_str().unwrap()) };
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };

    let result = MultiConfig::load();
    assert!(result.model_router.is_some(), "simple format must produce a model router");

    let router = result.model_router.unwrap();
    let r = router.read().unwrap();
    assert!(r.has_model("gpt-4o"));
    assert!(r.has_model("gpt-4o-mini"));
    assert_eq!(
        r.strategy(),
        anyllm_proxy::config::model_router::RoutingStrategy::LeastBusy
    );

    unsafe {
        std::env::remove_var("PROXY_CONFIG");
        std::env::remove_var("OPENAI_API_KEY");
    };
}

#[test]
fn load_litellm_format_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("litellm.yaml");
    std::fs::write(
        &path,
        r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
"#,
    )
    .unwrap();

    unsafe { std::env::set_var("PROXY_CONFIG", path.to_str().unwrap()) };

    let result = MultiConfig::load();
    assert!(result.model_router.is_some());
    let router = result.model_router.unwrap();
    assert!(router.read().unwrap().has_model("gpt-4o"));

    unsafe { std::env::remove_var("PROXY_CONFIG") };
}
```

Add `tempfile` to `[dev-dependencies]` in `crates/proxy/Cargo.toml` if not already present:

```toml
[dev-dependencies]
tempfile = "3"
```

Check first: `grep -n "tempfile" crates/proxy/Cargo.toml`

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test -p anyllm_proxy --test simple_config 2>&1 | head -20
```

Expected: `error: test file not found` or test fails because simple format falls through to LiteLLM parser.

- [ ] **Step 3: Modify `MultiConfig::load()` to detect `models:` key**

In `crates/proxy/src/config/mod.rs`, replace lines 492-537 with:

```rust
    pub fn load() -> LoadResult {
        if let Ok(path) = std::env::var("PROXY_CONFIG") {
            if path.ends_with(".yaml") || path.ends_with(".yml") {
                let yaml = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read config '{path}': {e}"));

                // Detect format: "models:" key = simple format, "model_list:" = LiteLLM.
                let probe: serde_yaml::Value = serde_yaml::from_str(&yaml)
                    .unwrap_or_else(|e| panic!("invalid YAML in '{path}': {e}"));

                if probe.get("models").is_some() {
                    // Simple native format.
                    let parsed = simple::parse_simple_yaml(&yaml);
                    return LoadResult {
                        multi_config: parsed.multi_config,
                        model_router: Some(Arc::new(std::sync::RwLock::new(parsed.router))),
                        litellm_master_key: None,
                    };
                }

                // LiteLLM format (model_list: + litellm_params:).
                let parsed = litellm::parse_litellm_yaml(&yaml);

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

                return LoadResult {
                    multi_config: parsed.multi_config,
                    model_router: Some(Arc::new(std::sync::RwLock::new(parsed.router))),
                    litellm_master_key: parsed.master_key,
                };
            }
            LoadResult {
                multi_config: Self::from_toml_file(&path),
                model_router: None,
                litellm_master_key: None,
            }
        } else {
            LoadResult {
                multi_config: Self::from_legacy_env(),
                model_router: None,
                litellm_master_key: None,
            }
        }
    }
```

- [ ] **Step 4: Run integration tests**

```bash
cargo test -p anyllm_proxy --test simple_config 2>&1
```

Expected: both tests pass.

- [ ] **Step 5: Run full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass, 0 failed.

- [ ] **Step 6: Clippy clean**

```bash
cargo clippy -p anyllm_proxy -- -D warnings 2>&1 | grep "^error" | head -20
```

Expected: no output (no errors).

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/config/mod.rs crates/proxy/tests/simple_config.rs crates/proxy/Cargo.toml
git commit -m "feat(config): wire simple YAML format into MultiConfig::load via 'models:' key detection"
```

---

## Task 4: Vertex base URL — fix Box::leak in `default_base_url`

The Vertex branch in `default_base_url` uses `Box::leak` to produce a `&'static str` from an env var. This is a memory leak acceptable in a startup path but not ideal. Replace with `String` ownership throughout.

**Files:**
- Modify: `crates/proxy/src/config/simple.rs` — `default_base_url` and `default_api_key_for_provider`

- [ ] **Step 1: Refactor `default_base_url` to return owned `String` without leaking**

The function already returns `String`, so the `Box::leak` pattern is only used to coerce a local `String` into a `&str` for use in format strings. Refactor to capture the value first:

```rust
fn default_base_url(kind: &BackendKind, entry: &NormalizedEntry) -> String {
    match kind {
        BackendKind::OpenAI => std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string()),
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
                    std::env::var("VERTEX_PROJECT")
                        .expect("project field (or VERTEX_PROJECT env var) required for vertex provider")
                });
            let region = entry
                .region
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::var("VERTEX_REGION")
                        .expect("region field (or VERTEX_REGION env var) required for vertex provider")
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
                    std::env::var("AZURE_OPENAI_ENDPOINT")
                        .expect("api_base field (or AZURE_OPENAI_ENDPOINT env var) required for azure provider")
                });
            let deployment = entry
                .deployment
                .as_deref()
                .unwrap_or("chat");
            let version = entry
                .api_version
                .as_deref()
                .unwrap_or("2024-10-21");
            format!(
                "{}/openai/deployments/{deployment}/chat/completions?api-version={version}",
                endpoint.trim_end_matches('/')
            )
        }
        BackendKind::Bedrock => {
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
```

Replace the entire previous `default_base_url` function with the above (Task 3's `simple.rs` had `Box::leak` for Vertex/Azure; this removes it).

- [ ] **Step 2: Run clippy**

```bash
cargo clippy -p anyllm_proxy -- -D warnings 2>&1 | grep "^error" | head -10
```

Expected: no errors.

- [ ] **Step 3: Run all tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/config/simple.rs
git commit -m "fix(config/simple): remove Box::leak in default_base_url, use owned Strings"
```

---

## Task 5: Update CLAUDE.md with simple config format

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add simple config to CLAUDE.md under Environment Variables**

Find the `PROXY_CONFIG` entry in CLAUDE.md and update its description to mention both formats:

Old:
```
- `PROXY_CONFIG`: Path to config file. TOML for multi-backend config, or `.yaml`/`.yml` for LiteLLM-compatible config with model_list routing.
```

New:
```
- `PROXY_CONFIG`: Path to config file. Accepted formats:
  - **Simple YAML** (`.yaml`/`.yml` with top-level `models:` key): native format, provider API keys from env vars, inline `routing_strategy`, string shorthand (`- openai/gpt-4o`). See docs below.
  - **LiteLLM YAML** (`.yaml`/`.yml` with top-level `model_list:` key): LiteLLM-compatible format.
  - **TOML** (any other extension): multi-backend TOML config.
```

Also add a new section **Simple Config Format** under the Environment Variables block, before "References":

```markdown
## Simple Config Format

Simpler alternative to the LiteLLM format. Activated when the YAML file has a top-level `models:` key.

```yaml
# anyllm.yaml
routing_strategy: latency-based   # round-robin (default) | least-busy | latency-based | weighted | cost-based
listen_port: 3000                  # optional
log_bodies: false                  # optional

models:
  # String shorthand: bare model name defaults to openai
  - gpt-4o
  # String shorthand with provider prefix
  - openai/gpt-4o-mini
  - anthropic/claude-3-5-sonnet-20241022
  # Full form: virtual name, actual model, weight, limits
  - name: smart                    # virtual name sent by clients
    model: gpt-4o
    provider: openai
    weight: 3
    rpm: 1000
    tpm: 500000
  - name: smart                    # second deployment for "smart" = round-robin/failover
    model: claude-3-5-sonnet-20241022
    provider: anthropic
    weight: 1
```

API key defaults per provider (used when `api_key` is not specified in the entry):

| provider   | env var                                        |
|------------|------------------------------------------------|
| openai     | `OPENAI_API_KEY`                               |
| anthropic  | `ANTHROPIC_API_KEY`                            |
| gemini     | `GEMINI_API_KEY`                               |
| vertex     | `VERTEX_API_KEY` or `GOOGLE_ACCESS_TOKEN`      |
| azure      | `AZURE_OPENAI_API_KEY`                         |
| bedrock    | `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY`  |
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: document simple YAML config format in CLAUDE.md"
```

---

## Self-Review

**Spec coverage check:**

| Requirement | Task |
|-------------|------|
| Simple YAML/JSON routing config | Task 1 (serde types), Task 2 (parser), Task 3 (dispatch) |
| Weighted routing | Task 2 (`weight:` field, `routing_strategy: weighted`) |
| Latency-based routing | Task 2 (`routing_strategy: latency-based`) |
| Provider env-var defaults | Task 2 (`default_api_key_for_provider`) |
| Backward compat with LiteLLM format | Task 3 (detection by `models:` vs `model_list:` key) |
| Backward compat with TOML format | Task 3 (non-YAML path unchanged) |
| No regressions | Task 2 step 6, Task 3 step 5 |
| Docs | Task 5 |

**Placeholder scan:** No TBDs, no "add appropriate error handling" phrases, no "similar to Task N" references. All code blocks are complete.

**Type consistency:** `SimpleParsed`, `SimpleConfig`, `SimpleModelEntry`, `SimpleModelFull`, `NormalizedEntry` — names are used consistently across Tasks 1-3. `parse_simple_yaml` is the single entry point referenced in Task 3's `load()`. `parse_routing_strategy_str` renamed in Task 2 step 4 and referenced correctly.

**One gap found:** `BackendKind` does not implement `PartialEq` for `kind != BackendKind::Bedrock` check in `build_backend_config`. Check with `grep -n "PartialEq" crates/proxy/src/config/mod.rs` — it is derived for `BackendKind`. No issue.

**Azure `api_base` double-use:** In `build_backend_config`, when `kind == AzureOpenAI`, the `base_url` parameter already contains the full deployment URL (built by `default_base_url`). The function should use `base_url` directly, not re-read `entry.api_base`. This is consistent with the LiteLLM path. Confirmed: `build_backend_config` uses `base_url` as-is for Azure (same as LiteLLM's `effective_url`). No issue.
