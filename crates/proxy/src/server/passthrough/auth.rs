use crate::server::middleware::ClientAuthPath;
use axum::http::HeaderMap;

/// Selects the exact incoming credential to forward upstream when
/// `ANTHROPIC_FORWARD_CLIENT_AUTH` is enabled. Same precedence as
/// `validate_auth` (`x-api-key` / `x-goog-api-key` win over `authorization`,
/// see `server/middleware/auth.rs`'s `api_key = headers.get("x-api-key")
/// .or_else(|| headers.get("x-goog-api-key"))`) so this always forwards the
/// credential that actually gated the request into the proxy, never a
/// second, unrelated header the client also happened to send.
///
/// `x-goog-api-key` (Gemini-CLI compatibility) is folded into the
/// `x-api-key` slot rather than forwarded under its own name: Anthropic's API
/// only recognizes `x-api-key`/`authorization`, so a client authenticated via
/// `x-goog-api-key` must still have its value sent upstream as `x-api-key`,
/// not as a header name Anthropic would silently ignore. Beyond that one
/// rename, no shape detection/conversion happens -- forwarded byte-for-byte,
/// unlike LiteLLM's `optionally_handle_anthropic_oauth()`, which mis-converts
/// a Bearer token into `x-api-key`.
fn select_client_auth_override(headers: &HeaderMap) -> Option<(&'static str, &str)> {
    if let Some(v) = headers
        .get("x-api-key")
        .or_else(|| headers.get("x-goog-api-key"))
        .and_then(|v| v.to_str().ok())
    {
        if !v.is_empty() {
            return Some(("x-api-key", v));
        }
    }
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if !v.is_empty() {
            return Some(("authorization", v));
        }
    }
    None
}

/// Only `StaticKey`/`OpenRelay` mean "the credential that gated this request
/// IS the operator's own secret" (a single-key/BYOK deployment). A virtual
/// key is deliberately not a real Anthropic credential and a JWT is a
/// proxy-auth artifact, so those must never be forwarded upstream regardless
/// of the `ANTHROPIC_FORWARD_CLIENT_AUTH` toggle.
fn client_auth_forwardable(auth_path: Option<ClientAuthPath>) -> bool {
    matches!(
        auth_path,
        Some(ClientAuthPath::StaticKey) | Some(ClientAuthPath::OpenRelay)
    )
}

/// Resolves the client-credential override shared by both passthrough
/// handlers. `vk_ctx`/`claims` are checked directly, not just via
/// `client_auth_forwardable(auth_path)`: `ClientAuthPath` and
/// `VirtualKeyContext`/`JwtClaims` are inserted as two independent
/// `request.extensions_mut().insert()` calls in `validate_auth`, with
/// nothing structurally coupling them, so a future edit to one of those
/// branches could desync them without a compile error. Re-checking presence
/// of the extension that actually gates virtual-key/OIDC requests fails
/// closed instead of silently forwarding a non-operator credential if that
/// ever happens.
pub(crate) fn resolve_client_auth_override<'h>(
    forward_client_auth: bool,
    auth_path: Option<ClientAuthPath>,
    vk_ctx: &Option<crate::server::middleware::VirtualKeyContext>,
    claims: &Option<crate::server::oidc::JwtClaims>,
    headers: &'h HeaderMap,
) -> Option<(&'static str, &'h str)> {
    if forward_client_auth
        && client_auth_forwardable(auth_path)
        && vk_ctx.is_none()
        && claims.is_none()
    {
        select_client_auth_override(headers)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        client_auth_forwardable, resolve_client_auth_override, select_client_auth_override,
        ClientAuthPath,
    };
    use axum::http::HeaderMap;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn selects_x_api_key_when_only_that_is_sent() {
        let h = headers(&[("x-api-key", "client-key")]);
        assert_eq!(
            select_client_auth_override(&h),
            Some(("x-api-key", "client-key"))
        );
    }

    #[test]
    fn selects_authorization_when_only_that_is_sent() {
        let h = headers(&[("authorization", "Bearer sk-ant-oat-abc")]);
        assert_eq!(
            select_client_auth_override(&h),
            Some(("authorization", "Bearer sk-ant-oat-abc"))
        );
    }

    #[test]
    fn prefers_x_api_key_when_both_sent_matching_validate_auth_precedence() {
        let h = headers(&[
            ("x-api-key", "client-key"),
            ("authorization", "Bearer sk-ant-oat-abc"),
        ]);
        assert_eq!(
            select_client_auth_override(&h),
            Some(("x-api-key", "client-key"))
        );
    }

    #[test]
    fn returns_none_when_neither_header_sent() {
        let h = headers(&[]);
        assert_eq!(select_client_auth_override(&h), None);
    }

    #[test]
    fn selects_x_goog_api_key_forwarded_as_x_api_key() {
        // validate_auth (server/middleware/auth.rs) treats x-goog-api-key as
        // fully equivalent to x-api-key for authentication, but Anthropic's
        // API only understands x-api-key -- the value must be forwarded
        // under the x-api-key name, not the literal x-goog-api-key name.
        let h = headers(&[("x-goog-api-key", "gemini-cli-key")]);
        assert_eq!(
            select_client_auth_override(&h),
            Some(("x-api-key", "gemini-cli-key"))
        );
    }

    #[test]
    fn prefers_x_api_key_over_x_goog_api_key_matching_validate_auth_precedence() {
        let h = headers(&[
            ("x-api-key", "primary-key"),
            ("x-goog-api-key", "secondary-key"),
        ]);
        assert_eq!(
            select_client_auth_override(&h),
            Some(("x-api-key", "primary-key"))
        );
    }

    #[test]
    fn client_auth_forwardable_only_for_static_key_and_open_relay() {
        assert!(client_auth_forwardable(Some(ClientAuthPath::StaticKey)));
        assert!(client_auth_forwardable(Some(ClientAuthPath::OpenRelay)));
        assert!(!client_auth_forwardable(Some(ClientAuthPath::VirtualKey)));
        assert!(!client_auth_forwardable(Some(ClientAuthPath::OidcJwt)));
        assert!(!client_auth_forwardable(None));
    }

    #[test]
    fn resolve_client_auth_override_forwards_on_static_key() {
        let h = headers(&[("x-api-key", "client-key")]);
        assert_eq!(
            resolve_client_auth_override(true, Some(ClientAuthPath::StaticKey), &None, &None, &h),
            Some(("x-api-key", "client-key"))
        );
    }

    #[test]
    fn resolve_client_auth_override_refuses_when_vk_ctx_present_even_if_auth_path_says_static_key()
    {
        // Regression guard for the ClientAuthPath/VirtualKeyContext desync
        // risk: even if a future bug leaves auth_path reporting StaticKey
        // while a VirtualKeyContext extension is also present, forwarding
        // must still be refused.
        let h = headers(&[("x-api-key", "client-key")]);
        let vk_ctx = Some(crate::server::middleware::VirtualKeyContext {
            key_id: 1,
            #[cfg(feature = "redis")]
            key_hash_hex: String::new(),
            rate_state: std::sync::Arc::new(crate::admin::keys::RateLimitState::new()),
            allowed_models: None,
            allowed_routes: None,
            period_reset: None,
        });
        assert_eq!(
            resolve_client_auth_override(true, Some(ClientAuthPath::StaticKey), &vk_ctx, &None, &h),
            None
        );
    }

    #[test]
    fn resolve_client_auth_override_refuses_when_feature_disabled() {
        let h = headers(&[("x-api-key", "client-key")]);
        assert_eq!(
            resolve_client_auth_override(false, Some(ClientAuthPath::StaticKey), &None, &None, &h),
            None
        );
    }
}
