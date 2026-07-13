use super::*;

#[test]
fn parse_auth_mode_new_names() {
    assert_eq!(AuthMode::from_env_str("oidc"), AuthMode::OidcOnly);
    assert_eq!(AuthMode::from_env_str("oidc-only"), AuthMode::OidcOnly);
    assert_eq!(AuthMode::from_env_str("oidc_only"), AuthMode::OidcOnly);
    assert_eq!(AuthMode::from_env_str("keys"), AuthMode::KeysOnly);
    assert_eq!(AuthMode::from_env_str("keys-only"), AuthMode::KeysOnly);
    assert_eq!(AuthMode::from_env_str("keys_only"), AuthMode::KeysOnly);
    assert_eq!(AuthMode::from_env_str("both"), AuthMode::Both);
}

#[test]
fn parse_auth_mode_legacy_names() {
    assert_eq!(AuthMode::from_env_str("jwt_only"), AuthMode::OidcOnly);
    assert_eq!(AuthMode::from_env_str("jwt_or_keys"), AuthMode::Both);
    assert_eq!(AuthMode::from_env_str("JWT_ONLY"), AuthMode::OidcOnly);
}

#[test]
fn parse_auth_mode_unknown_defaults_to_both() {
    assert_eq!(AuthMode::from_env_str("unknown"), AuthMode::Both);
    assert_eq!(AuthMode::from_env_str(""), AuthMode::Both);
}

#[test]
fn auth_mode_oidc_only() {
    assert!(AuthMode::OidcOnly.allows_oidc());
    assert!(!AuthMode::OidcOnly.allows_key_auth());
}

#[test]
fn auth_mode_keys_only() {
    assert!(AuthMode::KeysOnly.allows_key_auth());
    assert!(!AuthMode::KeysOnly.allows_oidc());
}

#[test]
fn auth_mode_both_allows_all() {
    assert!(AuthMode::Both.allows_oidc());
    assert!(AuthMode::Both.allows_key_auth());
}

#[test]
fn auth_mode_from_env_defaults_to_both() {
    let mode = AuthMode::from_env_str("unrecognized_value");
    assert_eq!(mode, AuthMode::Both);
}

#[test]
fn forward_client_auth_rejects_multiple_static_keys_without_open_relay() {
    assert!(forward_client_auth_misconfigured(2, false));
}

#[test]
fn forward_client_auth_allows_open_relay_even_with_multiple_keys() {
    // Reflects that `open_relay_active()` can only be true when
    // `distinct_static_key_count()` is 0 (see the OPEN_RELAY static) --
    // this combination is unreachable via those two real accessors, but
    // the pure decision function itself must still handle it sanely.
    assert!(!forward_client_auth_misconfigured(2, true));
}

#[test]
fn forward_client_auth_allows_exactly_one_key() {
    assert!(!forward_client_auth_misconfigured(1, false));
}

#[test]
fn forward_client_auth_allows_zero_keys() {
    assert!(!forward_client_auth_misconfigured(0, false));
}
