// YAML-based fallback chain configuration.
// Loaded from the FALLBACK_CONFIG env var (path to a YAML file).

use serde::Deserialize;
use std::collections::HashMap;

/// Top-level fallback configuration, deserialized from YAML.
///
/// Example:
/// ```yaml
/// fallback_chains:
///   default:
///     - name: azure
///       env_prefix: AZURE_FALLBACK_
///     - name: openai
///       env_prefix: OPENAI_FALLBACK_
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct FallbackConfig {
    pub fallback_chains: HashMap<String, Vec<BackendSpec>>,
}

/// A single backend entry in a fallback chain.
/// `name` is a human-readable label (used in logs).
/// `env_prefix` identifies which env vars configure this backend.
#[derive(Debug, Clone, Deserialize)]
pub struct BackendSpec {
    pub name: String,
    pub env_prefix: String,
}

/// Parse fallback config from a YAML string.
pub fn parse_fallback_config(yaml: &str) -> Result<FallbackConfig, serde_yaml::Error> {
    serde_yaml::from_str(yaml)
}

/// Load fallback config from the file path in `FALLBACK_CONFIG` env var.
/// Returns `None` if the env var is not set.
/// Returns `Err` if the file cannot be read or parsed.
pub fn load_fallback_config() -> Result<Option<FallbackConfig>, FallbackConfigError> {
    let path = match std::env::var("FALLBACK_CONFIG") {
        Ok(p) if !p.is_empty() => p,
        _ => return Ok(None),
    };

    let contents = std::fs::read_to_string(&path).map_err(|e| FallbackConfigError::Io {
        path: path.clone(),
        source: e,
    })?;

    let config =
        parse_fallback_config(&contents).map_err(|e| FallbackConfigError::Parse { source: e })?;

    Ok(Some(config))
}

/// Errors that can occur when loading fallback configuration.
#[derive(Debug)]
pub enum FallbackConfigError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse {
        source: serde_yaml::Error,
    },
}

impl std::fmt::Display for FallbackConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read fallback config at {path}: {source}")
            }
            Self::Parse { source } => write!(f, "failed to parse fallback config YAML: {source}"),
        }
    }
}

impl std::error::Error for FallbackConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        let yaml = r#"
fallback_chains:
  default:
    - name: azure
      env_prefix: AZURE_FALLBACK_
    - name: openai
      env_prefix: OPENAI_FALLBACK_
  high_priority:
    - name: vertex
      env_prefix: VERTEX_FALLBACK_
"#;
        let config = parse_fallback_config(yaml).expect("should parse valid YAML");
        assert_eq!(config.fallback_chains.len(), 2);

        let default_chain = &config.fallback_chains["default"];
        assert_eq!(default_chain.len(), 2);
        assert_eq!(default_chain[0].name, "azure");
        assert_eq!(default_chain[0].env_prefix, "AZURE_FALLBACK_");
        assert_eq!(default_chain[1].name, "openai");
        assert_eq!(default_chain[1].env_prefix, "OPENAI_FALLBACK_");
    }

    #[test]
    fn parse_empty_chains() {
        let yaml = "fallback_chains: {}\n";
        let config = parse_fallback_config(yaml).expect("should parse empty chains");
        assert!(config.fallback_chains.is_empty());
    }

    #[test]
    fn parse_malformed_yaml() {
        let yaml = "fallback_chains:\n  - this is wrong: [";
        let result = parse_fallback_config(yaml);
        assert!(result.is_err(), "malformed YAML should fail");
    }

    #[test]
    fn parse_missing_required_fields() {
        // Backend spec missing `env_prefix`
        let yaml = r#"
fallback_chains:
  default:
    - name: azure
"#;
        let result = parse_fallback_config(yaml);
        assert!(result.is_err(), "missing env_prefix should fail");
    }
}
