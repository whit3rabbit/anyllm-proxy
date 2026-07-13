use super::*;

fn custom_request(url: &str) -> DiscoverRequest {
    DiscoverRequest {
        source: "custom".to_string(),
        url: Some(url.to_string()),
        provider_id: None,
        api_key: None,
    }
}

#[test]
fn custom_discover_rejects_loopback_url() {
    let err = resolve_discover_target(&custom_request("http://127.0.0.1:11434"), false)
        .expect_err("loopback custom discovery URL must be rejected");

    assert!(err.contains("private/loopback"));
}

#[test]
fn custom_discover_rejects_file_scheme() {
    let err = resolve_discover_target(&custom_request("file:///tmp/anyllm"), false)
        .expect_err("non-http custom discovery URL must be rejected");

    assert!(err.contains("scheme"));
}

#[test]
fn custom_discover_accepts_https_url_and_appends_models() {
    let (url, api_key) = resolve_discover_target(&custom_request("https://1.1.1.1"), false)
        .expect("public HTTPS custom discovery URL should be accepted");

    assert_eq!(url, "https://1.1.1.1/v1/models");
    assert_eq!(api_key, None);
}

#[test]
fn custom_discover_accepts_existing_models_suffix() {
    let (url, api_key) =
        resolve_discover_target(&custom_request("https://1.1.1.1/v1/models"), false)
            .expect("public HTTPS custom discovery URL should be accepted");

    assert_eq!(url, "https://1.1.1.1/v1/models");
    assert_eq!(api_key, None);
}

#[test]
fn custom_discover_does_not_double_v1_suffix() {
    // Local-provider catalog defaults already end in /v1; don't produce /v1/v1/models.
    let (url, _) = resolve_discover_target(&custom_request("http://192.168.1.72:4444/v1"), true)
        .expect("local LAN /v1 discovery URL should be accepted when allow_local");
    assert_eq!(url, "http://192.168.1.72:4444/v1/models");

    // Trailing slash on a /v1 base collapses the same way.
    let (url, _) = resolve_discover_target(&custom_request("http://192.168.1.72:4444/v1/"), true)
        .expect("trailing-slash /v1 discovery URL should be accepted when allow_local");
    assert_eq!(url, "http://192.168.1.72:4444/v1/models");
}

#[test]
fn custom_discover_local_allows_loopback_but_keeps_scheme_check() {
    // allow_local=true: loopback is accepted (LM Studio/Ollama on localhost)...
    let (url, _) = resolve_discover_target(&custom_request("http://127.0.0.1:1234"), true)
        .expect("local loopback discovery URL should be accepted when allow_local");
    assert_eq!(url, "http://127.0.0.1:1234/v1/models");

    // ...but a non-http scheme is still rejected.
    let err = resolve_discover_target(&custom_request("file:///tmp/anyllm"), true)
        .expect_err("non-http scheme must be rejected even when allow_local");
    assert!(err.contains("scheme"));
}

#[test]
fn custom_discover_threads_api_key() {
    let mut req = custom_request("http://192.168.1.72:4444");
    req.api_key = Some("sk-local".to_string());
    let (_, api_key) = resolve_discover_target(&req, true)
        .expect("local LAN discovery URL should be accepted when allow_local");
    assert_eq!(api_key.as_deref(), Some("sk-local"));
}

#[test]
fn custom_discover_local_rejects_public_and_metadata_hosts() {
    // allow_local must NOT permit public hosts (would be a general outbound fetch).
    let err = resolve_discover_target(&custom_request("http://1.1.1.1/v1"), true)
        .expect_err("public IP must be rejected even when allow_local");
    assert!(err.contains("loopback/LAN"));
    // Cloud-metadata / link-local stay blocked here too.
    let err = resolve_discover_target(&custom_request("http://169.254.169.254/"), true)
        .expect_err("metadata IP must be rejected even when allow_local");
    assert!(err.contains("loopback/LAN"));
    // A bare host name that isn't `localhost` cannot be verified as local.
    let err = resolve_discover_target(&custom_request("http://evil.example.com/"), true)
        .expect_err("non-local host name must be rejected when allow_local");
    assert!(err.contains("loopback/LAN"));
}
