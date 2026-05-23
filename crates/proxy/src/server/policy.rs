//! Request policy enforcement.
//!
//! Enforces per-key restrictions (e.g., model allowlists, route scoping) before
//! the request reaches the backend. Policies are optional: absent means "allow all".

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

/// Check if a route ID is allowed by the key's route scoping policy.
/// Returns true if no policy is set (all routes allowed).
/// Route IDs are UUIDs, so this is exact-match only.
pub fn is_route_allowed(route_id: &str, allowed_routes: &Option<Vec<String>>) -> bool {
    let Some(allowed) = allowed_routes else {
        return true;
    };
    allowed.iter().any(|r| r == route_id)
}

#[derive(Debug)]
pub(crate) struct RouteScopeError;

impl RouteScopeError {
    pub(crate) fn message(&self) -> &'static str {
        "This API key is not allowed to access this route."
    }
}

/// Enforce virtual-key route scoping for a resolved backend name.
///
/// Scoped keys fail closed when shared route state cannot be resolved. Route
/// IDs are the operator-facing `routes.id` UUIDs stored in `allowed_routes`.
pub(crate) async fn enforce_route_scope(
    backend_name: &str,
    shared: &Option<crate::admin::state::SharedState>,
    allowed_routes: &Option<Vec<String>>,
) -> Result<(), RouteScopeError> {
    let Some(allowed) = allowed_routes else {
        return Ok(());
    };

    if allowed.is_empty() {
        tracing::warn!(backend_name, "route scope denied by empty allowlist");
        return Err(RouteScopeError);
    }

    let Some(shared) = shared else {
        tracing::warn!(
            backend_name,
            "route scope denied because shared state is unavailable"
        );
        return Err(RouteScopeError);
    };

    let backend = backend_name.to_string();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::enabled_route_ids_for_backend_name(conn, &backend)
    })
    .await;

    match result {
        Some(Ok(route_ids)) => {
            if route_ids.iter().any(|route_id| allowed.contains(route_id)) {
                Ok(())
            } else {
                tracing::warn!(
                    backend_name,
                    route_count = route_ids.len(),
                    "route scope denied because backend is not in an allowed route"
                );
                Err(RouteScopeError)
            }
        }
        Some(Err(error)) => {
            tracing::error!(
                backend_name,
                %error,
                "route scope denied because route lookup failed"
            );
            Err(RouteScopeError)
        }
        None => {
            tracing::error!(
                backend_name,
                "route scope denied because route lookup task failed"
            );
            Err(RouteScopeError)
        }
    }
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

    // ── Route scoping tests ──────────────────────────────────────────────────

    #[test]
    fn route_allowed_when_no_policy() {
        assert!(is_route_allowed("any-route-id", &None));
    }

    #[test]
    fn route_allowed_exact_match() {
        let policy = Some(vec!["route-abc".to_string(), "route-def".to_string()]);
        assert!(is_route_allowed("route-abc", &policy));
        assert!(is_route_allowed("route-def", &policy));
        assert!(!is_route_allowed("route-xyz", &policy));
    }

    #[test]
    fn route_empty_allowlist_denies_all() {
        let policy = Some(vec![]);
        assert!(!is_route_allowed("route-abc", &policy));
    }
}
