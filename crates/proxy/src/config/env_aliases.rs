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
];

/// Set anyllm-proxy env vars from LiteLLM equivalents when the target is unset.
///
/// # Safety
/// Must be called before any config parsing and before spawning threads
/// (std::env::set_var is not thread-safe on all platforms).
pub fn apply_env_aliases() {
    for &(from, to) in ALIASES {
        if std::env::var(to).is_err() {
            if let Ok(val) = std::env::var(from) {
                // SAFETY: called single-threaded at startup before tokio runtime.
                unsafe { std::env::set_var(to, &val) };
                tracing::debug!(from = %from, to = %to, "applied LiteLLM env var alias");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serial test lock: env var mutations are process-global.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn alias_applied_when_target_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        // Clean slate
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

    #[test]
    fn alias_not_applied_when_target_already_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("PROXY_API_KEYS", "existing-key");
            std::env::set_var("LITELLM_MASTER_KEY", "sk-litellm");
        }

        apply_env_aliases();

        assert_eq!(std::env::var("PROXY_API_KEYS").unwrap(), "existing-key");

        // Cleanup
        unsafe {
            std::env::remove_var("PROXY_API_KEYS");
            std::env::remove_var("LITELLM_MASTER_KEY");
        }
    }
}
