//! Simple native YAML config format for anyllm-proxy.
//!
//! Activated when the config file contains a top-level `models:` key
//! (as opposed to LiteLLM's `model_list:`).

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
