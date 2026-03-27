//! Request policy enforcement.
//!
//! Enforces per-key restrictions (e.g., model allowlists) before the request
//! reaches the backend. Policies are optional: absent means "allow all".

/// Check if a model name is allowed by the key's policy.
/// Returns true if no policy is set (all models allowed).
///
/// Supported patterns:
/// - `"*"` -- allows any model
/// - `"claude-*"` -- prefix wildcard (matches `claude-3-opus`, `claude-sonnet-4-6`, etc.)
/// - `"gpt-4o"` -- exact match
pub fn is_model_allowed(model: &str, allowed_models: &Option<Vec<String>>) -> bool {
    let Some(allowed) = allowed_models else {
        return true;
    };
    for pattern in allowed {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            if model.starts_with(prefix) {
                return true;
            }
        } else if pattern == model {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_allowed_when_no_policy() {
        assert!(is_model_allowed("anything", &None));
    }

    #[test]
    fn model_allowed_exact_match() {
        let policy = Some(vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]);
        assert!(is_model_allowed("gpt-4o", &policy));
        assert!(is_model_allowed("gpt-4o-mini", &policy));
        assert!(!is_model_allowed("gpt-4", &policy));
    }

    #[test]
    fn model_allowed_wildcard() {
        let policy = Some(vec!["claude-*".to_string()]);
        assert!(is_model_allowed("claude-sonnet-4-6", &policy));
        assert!(is_model_allowed("claude-3-opus", &policy));
        assert!(!is_model_allowed("gpt-4o", &policy));
    }

    #[test]
    fn model_allowed_star_allows_all() {
        let policy = Some(vec!["*".to_string()]);
        assert!(is_model_allowed("literally-anything", &policy));
    }

    #[test]
    fn model_denied_when_not_in_list() {
        let policy = Some(vec!["gpt-4o".to_string()]);
        assert!(!is_model_allowed("gpt-4o-mini", &policy));
        assert!(!is_model_allowed("claude-sonnet-4-6", &policy));
    }

    #[test]
    fn model_empty_allowlist_denies_all() {
        let policy = Some(vec![]);
        assert!(!is_model_allowed("gpt-4o", &policy));
    }

    #[test]
    fn model_multiple_patterns() {
        let policy = Some(vec!["gpt-4o".to_string(), "claude-*".to_string()]);
        assert!(is_model_allowed("gpt-4o", &policy));
        assert!(is_model_allowed("claude-sonnet-4-6", &policy));
        assert!(!is_model_allowed("gpt-4o-mini", &policy));
    }
}
