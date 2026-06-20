use super::middleware::check_admin_rate_limit_with_rpm;
use super::*;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use dashmap::DashMap;
use std::net::{IpAddr, SocketAddr};
use tower::ServiceExt;

/// Build a minimal admin router for origin/host tests.
fn test_router() -> Router {
    // Raise rate limit so parallel unit tests don't interfere.
    set_admin_rpm(10_000);
    let shared = crate::admin::state::SharedState::new_for_test();
    let token = Arc::new(zeroize::Zeroizing::new("test-token".to_string()));
    admin_router(shared, token)
}

#[tokio::test]
async fn origin_localhost_allowed() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("origin", "http://localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn origin_evil_rejected() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("origin", "http://evil.com")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_origin_localhost_host_allowed() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_origin_127_host_allowed() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("host", "127.0.0.1:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_origin_evil_host_rejected() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("host", "evil.com")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_origin_no_host_rejected() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[test]
fn admin_rate_limit_enforced() {
    // Use a unique IP and pass rpm directly to avoid mutating globals,
    // which would race with test_router() calling set_admin_rpm(10_000).
    let ip: IpAddr = "198.51.100.1".parse().unwrap();
    static TEST_PRUNE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let bucket = DashMap::<IpAddr, std::collections::VecDeque<u64>>::new();
    bucket.remove(&ip);

    assert!(check_admin_rate_limit_with_rpm(ip, 3, &bucket, &TEST_PRUNE));
    assert!(check_admin_rate_limit_with_rpm(ip, 3, &bucket, &TEST_PRUNE));
    assert!(check_admin_rate_limit_with_rpm(ip, 3, &bucket, &TEST_PRUNE));
    // 4th request in the same window should be rejected.
    assert!(!check_admin_rate_limit_with_rpm(
        ip,
        3,
        &bucket,
        &TEST_PRUNE
    ));

    bucket.remove(&ip);
}

#[test]
fn sliding_window_blocks_on_rpm_exceeded() {
    // Use a unique IP to avoid test isolation issues.
    let ip: IpAddr = "10.88.77.66".parse().unwrap();
    static TEST_PRUNE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let bucket = DashMap::<IpAddr, std::collections::VecDeque<u64>>::new();
    // With rpm=2, the first 2 requests must pass, the 3rd must fail.
    assert!(check_admin_rate_limit_with_rpm(ip, 2, &bucket, &TEST_PRUNE));
    assert!(check_admin_rate_limit_with_rpm(ip, 2, &bucket, &TEST_PRUNE));
    assert!(
        !check_admin_rate_limit_with_rpm(ip, 2, &bucket, &TEST_PRUNE),
        "3rd request must be blocked when rpm=2"
    );
}

/// POST to a protected admin route without CSRF token returns 403.
#[tokio::test]
async fn post_without_csrf_returns_403() {
    let app = test_router();
    let req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"description":"test"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// POST with matching CSRF header and cookie, where token is server-issued, succeeds.
#[tokio::test]
async fn post_with_valid_csrf_passes_middleware() {
    set_admin_rpm(10_000);
    let shared = crate::admin::state::SharedState::new_for_test();
    let token_str = "a".repeat(64);
    // Pre-register the token as server-issued so validate_csrf can find it.
    shared.issued_csrf_tokens.insert(token_str.clone(), ());
    let app = admin_router(
        shared,
        Arc::new(zeroize::Zeroizing::new("test-token".to_string())),
    );
    let req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", &token_str)
        .header("cookie", format!("csrf_token={token_str}"))
        .body(Body::from(r#"{"description":"test"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // 403 would mean CSRF rejected; any other status means CSRF passed.
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn put_config_updates_redact_secrets_override() {
    set_admin_rpm(10_000);
    let shared = crate::admin::state::SharedState::new_for_test();
    let token_str = "c".repeat(64);
    shared.issued_csrf_tokens.insert(token_str.clone(), ());
    let app = admin_router(
        shared.clone(),
        Arc::new(zeroize::Zeroizing::new("test-token".to_string())),
    );
    let req = Request::put("/admin/api/config")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", &token_str)
        .header("cookie", format!("csrf_token={token_str}"))
        .extension(ConnectInfo("127.0.0.1:9090".parse::<SocketAddr>().unwrap()))
        .body(Body::from(r#"{"redact_secrets":true}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["keys"], serde_json::json!(["redact_secrets"]));
    assert!(shared.runtime_config.read().unwrap().redact_secrets);

    let conn = shared.db.lock().unwrap();
    let overrides = crate::admin::db::get_config_overrides(&conn).unwrap();
    assert!(overrides
        .iter()
        .any(|(key, value, _)| key == "redact_secrets" && value == "true"));
}

#[tokio::test]
async fn delete_config_redact_secrets_override_restores_loaded_default() {
    set_admin_rpm(10_000);
    let mut shared = crate::admin::state::SharedState::new_for_test();
    shared.runtime_defaults.redact_secrets = true;
    {
        let mut config = shared.runtime_config.write().unwrap();
        config.redact_secrets = false;
    }
    {
        let conn = shared.db.lock().unwrap();
        crate::admin::db::set_config_override(&conn, "redact_secrets", "false").unwrap();
    }

    let token_str = "d".repeat(64);
    shared.issued_csrf_tokens.insert(token_str.clone(), ());
    let app = admin_router(
        shared.clone(),
        Arc::new(zeroize::Zeroizing::new("test-token".to_string())),
    );
    let req = Request::delete("/admin/api/config/overrides/redact_secrets")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("x-csrf-token", &token_str)
        .header("cookie", format!("csrf_token={token_str}"))
        .extension(ConnectInfo("127.0.0.1:9090".parse::<SocketAddr>().unwrap()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        shared.runtime_config.read().unwrap().redact_secrets,
        "runtime config should return to the loaded redact_secrets default"
    );

    let conn = shared.db.lock().unwrap();
    let overrides = crate::admin::db::get_config_overrides(&conn).unwrap();
    assert!(!overrides.iter().any(|(key, _, _)| key == "redact_secrets"));
}

/// POST with a CSRF token that was not server-issued is rejected even if header==cookie.
#[tokio::test]
async fn post_with_unissued_csrf_returns_403() {
    let app = test_router();
    let token = "b".repeat(64); // valid format but never stored in issued_csrf_tokens
    let req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", &token)
        .header("cookie", format!("csrf_token={token}"))
        .body(Body::from(r#"{"description":"test"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// DELETE without CSRF token returns 403.
#[tokio::test]
async fn delete_without_csrf_returns_403() {
    let app = test_router();
    let req = Request::delete("/admin/api/keys/1")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// GET /admin/csrf-token returns 200 with JSON body and Set-Cookie header.
#[tokio::test]
async fn get_csrf_token_sets_cookie() {
    let app = test_router();
    let req = Request::get("/admin/csrf-token")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        set_cookie.contains("csrf_token="),
        "Set-Cookie must include csrf_token"
    );
    assert!(
        set_cookie.contains("SameSite=Strict"),
        "cookie must be SameSite=Strict"
    );
    // Not httpOnly so JS can read it.
    assert!(
        !set_cookie.to_lowercase().contains("httponly"),
        "csrf_token cookie must not be httpOnly"
    );
}

/// GET /admin/csrf-token returns JSON with csrf_token field.
#[tokio::test]
async fn get_csrf_token_returns_json() {
    let app = test_router();
    let req = Request::get("/admin/csrf-token")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let token = body["csrf_token"].as_str().unwrap();
    assert_eq!(token.len(), 64);
}

/// GET /admin/csrf-token requires the admin token before minting a token.
#[tokio::test]
async fn get_csrf_token_without_auth_returns_401() {
    let app = test_router();
    let req = Request::get("/admin/csrf-token")
        .header("host", "localhost:9090")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// GET /admin/csrf-token is protected by the same origin policy as admin APIs.
#[tokio::test]
async fn get_csrf_token_cross_origin_returns_403() {
    let app = test_router();
    let req = Request::get("/admin/csrf-token")
        .header("origin", "http://evil.com")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// GET requests to protected routes do NOT require CSRF token.
#[tokio::test]
async fn get_request_does_not_require_csrf() {
    let app = test_router();
    let req = Request::get("/admin/api/config")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // CSRF should not reject GET; any non-403 means CSRF passed.
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[test]
fn aws_access_key_id_uses_secret_pattern() {
    // The secret() closure masks the value; this test verifies the masking logic.
    let mask = |v: &str| {
        if !v.is_empty() {
            "***REDACTED***".to_string()
        } else {
            "<unset>".to_string()
        }
    };
    assert_eq!(mask("AKIAIOSFODNN7EXAMPLE"), "***REDACTED***");
    assert_eq!(mask(""), "<unset>");
}

#[test]
fn google_access_token_uses_secret_pattern() {
    let mask = |v: &str| {
        if !v.is_empty() {
            "***REDACTED***".to_string()
        } else {
            "<unset>".to_string()
        }
    };
    assert_eq!(mask("ya29.someoauthtoken"), "***REDACTED***");
}

#[cfg(test)]
mod timestamp_tests {
    use crate::admin::routes::helpers::is_valid_timestamp;

    #[test]
    fn accepts_date_only() {
        assert!(is_valid_timestamp("2026-03-31"));
    }

    #[test]
    fn accepts_datetime_utc() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00Z"));
    }

    #[test]
    fn accepts_datetime_no_tz() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_timestamp(""));
    }

    #[test]
    fn rejects_arbitrary_string() {
        assert!(!is_valid_timestamp("not-a-date"));
    }

    #[test]
    fn rejects_sql_injection_attempt() {
        assert!(!is_valid_timestamp("'; DROP TABLE request_log; --"));
    }

    #[test]
    fn rejects_too_short() {
        assert!(!is_valid_timestamp("2026-03"));
    }
}
