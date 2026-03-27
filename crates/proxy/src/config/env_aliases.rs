// LiteLLM environment variable aliases.
//
// Maps LiteLLM env var names to their anyllm-proxy equivalents.
// Applied once at startup; the real environment always wins (aliases
// only take effect when the target var is not already set).

const ALIASES: &[(&str, &str)] = &[
    // Auth
    ("LITELLM_MASTER_KEY", "PROXY_API_KEYS"),
    // Config file
    ("LITELLM_CONFIG", "PROXY_CONFIG"),
    // Azure
    ("AZURE_API_KEY", "AZURE_OPENAI_API_KEY"),
    ("AZURE_API_BASE", "AZURE_OPENAI_ENDPOINT"),
    ("AZURE_API_VERSION", "AZURE_OPENAI_API_VERSION"),
    // AWS Bedrock
    ("AWS_REGION_NAME", "AWS_REGION"),
    // IP allowlisting
    ("LITELLM_IP_ALLOWLIST", "IP_ALLOWLIST"),
];

/// Compute env var overrides from LiteLLM aliases without mutating the environment.
///
/// Returns `(target_var, value)` pairs for each alias where the source is set
/// and the target is not. Caller is responsible for applying them via `set_var`.
pub fn compute_env_aliases() -> Vec<(&'static str, String)> {
    let mut overrides = Vec::new();
    for &(from, to) in ALIASES {
        if std::env::var(to).is_err() {
            if let Ok(val) = std::env::var(from) {
                tracing::debug!(from = %from, to = %to, "computed LiteLLM env var alias");
                overrides.push((to, val));
            }
        }
    }
    overrides
}

/// Set anyllm-proxy env vars from LiteLLM equivalents when the target is unset.
///
/// Convenience wrapper around `compute_env_aliases()` + `set_var`.
/// Kept for backward compatibility in tests; production code should use
/// `compute_env_aliases()` and apply overrides in the consolidated block.
#[allow(dead_code)]
pub fn apply_env_aliases() {
    for (key, val) in compute_env_aliases() {
        // SAFETY: caller must ensure single-threaded context.
        unsafe { std::env::set_var(key, &val) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serial test lock: env var mutations are process-global.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn compute_returns_overrides_when_target_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("PROXY_API_KEYS");
            std::env::set_var("LITELLM_MASTER_KEY", "sk-test-master");
        }

        let overrides = compute_env_aliases();

        // Should contain the PROXY_API_KEYS override.
        let found = overrides
            .iter()
            .find(|(k, _)| *k == "PROXY_API_KEYS")
            .map(|(_, v)| v.as_str());
        assert_eq!(found, Some("sk-test-master"));

        // Environment should NOT have been mutated by compute alone.
        assert!(std::env::var("PROXY_API_KEYS").is_err());

        // Cleanup
        unsafe {
            std::env::remove_var("LITELLM_MASTER_KEY");
        }
    }

    #[test]
    fn compute_skips_when_target_already_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("PROXY_API_KEYS", "existing-key");
            std::env::set_var("LITELLM_MASTER_KEY", "sk-litellm");
        }

        let overrides = compute_env_aliases();

        // Should NOT contain PROXY_API_KEYS since the target is already set.
        let found = overrides.iter().any(|(k, _)| *k == "PROXY_API_KEYS");
        assert!(!found);

        // Cleanup
        unsafe {
            std::env::remove_var("PROXY_API_KEYS");
            std::env::remove_var("LITELLM_MASTER_KEY");
        }
    }

    #[test]
    fn apply_env_aliases_sets_vars() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("PROXY_API_KEYS");
            std::env::set_var("LITELLM_MASTER_KEY", "sk-test-master");
        }

        apply_env_aliases();

        assert_eq!(std::env::var("PROXY_API_KEYS").unwrap(), "sk-test-master");

        // Cleanup
        unsafe {
            std::env::remove_var("PROXY_API_KEYS");
            std::env::remove_var("LITELLM_MASTER_KEY");
        }
    }
}
