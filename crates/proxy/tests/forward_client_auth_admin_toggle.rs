// Regression test for the ANTHROPIC_FORWARD_CLIENT_AUTH misconfiguration
// guard when the toggle is flipped live via the admin API (not just at
// startup). See server/middleware/auth.rs::forward_client_auth_misconfigured
// and admin/routes/config.rs::put_config.
//
// This lives in its own integration test binary (its own process) rather
// than crates/proxy/src/admin/routes/tests.rs deliberately: the check reads
// server/middleware/auth.rs's ALLOWED_KEY_HASHES/OPEN_RELAY `LazyLock`
// statics, which evaluate ONCE per process on first access and are cached
// forever after. That file's shared `--lib` test binary already has other
// tests (e.g. config::env_aliases's) that mutate PROXY_API_KEYS, and its own
// existing forward_client_auth-adjacent tests never need PROXY_API_KEYS set
// with 2+ entries -- adding that scenario there would make the outcome
// depend on which test happens to touch those statics first. A dedicated
// binary guarantees this test is the first (and only) thing to do so.

use anyllm_proxy::admin::state::SharedState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn put_config_rejects_forward_client_auth_when_misconfigured() {
    // No PROXY_OPEN_RELAY set, and 2 distinct PROXY_API_KEYS entries: exactly
    // the combination forward_client_auth_misconfigured rejects, matching the
    // startup safeguard's rule.
    std::env::remove_var("PROXY_OPEN_RELAY");
    std::env::set_var("PROXY_API_KEYS", "key-one,key-two");

    let shared = SharedState::new_for_test();
    let token_str = "k".repeat(64);
    shared.issued_csrf_tokens.insert(token_str.clone(), ());
    let app = anyllm_proxy::admin::routes::admin_router(
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
        .body(Body::from(r#"{"forward_client_auth":true}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        !shared.runtime_config.read().unwrap().forward_client_auth,
        "runtime config must not have been flipped on"
    );

    let conn = shared.db.lock().unwrap();
    let overrides = anyllm_proxy::admin::db::get_config_overrides(&conn).unwrap();
    assert!(
        !overrides
            .iter()
            .any(|(key, _, _)| key == "forward_client_auth"),
        "rejected toggle must not be persisted as an override"
    );
}
