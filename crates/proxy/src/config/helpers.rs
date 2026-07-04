/// Strip curly/smart quotes and other non-ASCII punctuation that copy-paste
/// from rich-text sources (Slack, docs, web pages) can silently inject into
/// API keys. Logs a warning so the operator notices.
pub fn sanitize_api_key(key: &str) -> String {
    // U+2018 ' U+2019 ' U+201C " U+201D "
    let cleaned: String = key
        .chars()
        .filter(|c| !matches!(c, '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}'))
        .collect();
    if cleaned.len() != key.len() {
        tracing::warn!(
            "stripped curly/smart quotes from API key \
             (likely copy-pasted from a rich-text source)"
        );
    }
    cleaned
}

/// Strip a trailing `/v1` or `/v1/` suffix from a base URL.
///
/// The OpenAI client always appends `/v1/chat/completions`, so provider URLs
/// that include `/v1` (e.g. `https://openrouter.ai/api/v1`) would produce a
/// doubled path without this.
pub fn strip_v1_suffix(url: &str) -> &str {
    url.strip_suffix("/v1/")
        .or_else(|| url.strip_suffix("/v1"))
        .unwrap_or(url)
}

/// Parse a boolean env var as `"true"` or `"1"`, defaulting to `false` if
/// unset. Shared by every `BackendConfig` loader that reads
/// `ANTHROPIC_FORWARD_CLIENT_AUTH` so the parsing rule lives in one place.
pub fn env_bool_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Resolve a config value that may reference an env var via `env:VAR_NAME` prefix.
/// This allows TOML config files to reference secrets from the environment
/// without hardcoding them, keeping credentials out of version control.
pub fn resolve_env_value(value: &str) -> Result<String, String> {
    if let Some(var_name) = value.strip_prefix("env:") {
        std::env::var(var_name)
            .map_err(|_| format!("env var '{var_name}' referenced in config is not set"))
    } else if let Some(var_name) = value.strip_prefix("os.environ/") {
        // LiteLLM-compatible syntax: "os.environ/VAR_NAME"
        std::env::var(var_name)
            .map_err(|_| format!("env var '{var_name}' (os.environ/ syntax) is not set"))
    } else {
        Ok(value.to_string())
    }
}

/// Extract the LiteLLM `general_settings.master_key` from the config file at
/// `path`, if present. Returns `None` for non-YAML files, non-LiteLLM formats,
/// missing files, or configs without a master key. This is intentionally
/// lightweight: it reads only enough to find the key, without full config parsing.
///
/// Designed to be called from `fn main()` (single-threaded, before the tokio
/// runtime) so the result can be applied via `set_var` without UB.
pub fn extract_litellm_master_key(path: &str) -> Option<String> {
    if !(path.ends_with(".yaml") || path.ends_with(".yml")) {
        return None;
    }
    let yaml = std::fs::read_to_string(path).ok()?;
    // Only LiteLLM format (model_list:) carries a master_key.
    let probe: serde_yaml::Value = serde_yaml::from_str(&yaml).ok()?;
    probe.get("model_list")?;
    super::litellm::extract_master_key(&yaml)
}

pub(crate) fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}
