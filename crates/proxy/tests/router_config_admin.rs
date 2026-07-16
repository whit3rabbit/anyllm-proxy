// Admin API coverage for the Claude Code tier router config (config key
// "router"): validation rejects tiers pointing at unknown managed backends,
// and a valid config round-trips through GET /admin/api/config.
// See admin/routes/config/{put,get}.rs and config/router_config.rs.

use anyllm_proxy::admin::state::SharedState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

fn admin_app(shared: &SharedState) -> axum::Router {
    anyllm_proxy::admin::routes::admin_router(
        shared.clone(),
        Arc::new(zeroize::Zeroizing::new("test-token".to_string())),
    )
}

fn put_router(token: &str, body: &str) -> Request<Body> {
    Request::put("/admin/api/config")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", token)
        .header("cookie", format!("csrf_token={token}"))
        .extension(ConnectInfo("127.0.0.1:9090".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn put_router_rejects_unknown_backend() {
    let shared = SharedState::new_for_test();
    let token = "k".repeat(64);
    shared.issued_csrf_tokens.insert(token.clone(), ());

    // No managed backends registered, so "ghost" is unknown -> 400.
    let body = r#"{"router":{"enabled":true,"default":{"backend_name":"ghost","model":"m","enabled":true}}}"#;
    let resp = admin_app(&shared)
        .oneshot(put_router(&token, body))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        !shared.runtime_config.read().unwrap().router.enabled,
        "rejected router config must not be applied"
    );
}

#[tokio::test]
async fn put_router_accepts_unset_tiers_and_round_trips() {
    let shared = SharedState::new_for_test();
    let token = "k".repeat(64);
    shared.issued_csrf_tokens.insert(token.clone(), ());

    // Empty backend_name = unset, allowed even with no managed backends.
    let body = r#"{"router":{"enabled":true,"context_threshold":12345}}"#;
    let resp = admin_app(&shared)
        .oneshot(put_router(&token, body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    {
        let cfg = shared.runtime_config.read().unwrap();
        assert!(cfg.router.enabled);
        assert_eq!(cfg.router.context_threshold, 12345);
    }

    // GET returns the persisted router blob.
    let get = Request::get("/admin/api/config")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-token")
        .extension(ConnectInfo("127.0.0.1:9090".parse::<SocketAddr>().unwrap()))
        .body(Body::empty())
        .unwrap();
    let resp = admin_app(&shared).oneshot(get).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["router"]["enabled"], true);
    assert_eq!(json["router"]["context_threshold"], 12345);
}

#[tokio::test]
async fn put_router_accepts_statically_configured_backend() {
    let mut shared = SharedState::new_for_test();
    // Simulate a YAML/TOML/LiteLLM backend reachable via AppState.all_backends
    // (populated only under a model router). No managed backend of this name.
    shared.static_backends = Arc::new(std::collections::HashSet::from(["yaml-be".to_string()]));
    let token = "k".repeat(64);
    shared.issued_csrf_tokens.insert(token.clone(), ());

    // Enabled tier points at the static-config backend -> accepted, not 400.
    let body = r#"{"router":{"enabled":true,"default":{"backend_name":"yaml-be","model":"m","enabled":true}}}"#;
    let resp = admin_app(&shared)
        .oneshot(put_router(&token, body))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let cfg = shared.runtime_config.read().unwrap();
    assert!(cfg.router.enabled);
    assert_eq!(cfg.router.default.backend_name, "yaml-be");
}
