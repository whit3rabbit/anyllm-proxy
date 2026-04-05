// Integration tests for virtual key admin API (T038), rate limiting (T051),
// budget enforcement (US5), and RBAC (US6).
//
// Admin routes require CSRF double-submit cookie protection on POST/PUT/DELETE.
// Tests inject a fixed test token via X-CSRF-Token header + Cookie to satisfy the middleware.

use anyllm_proxy::admin;
use anyllm_proxy::config::{BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::Request;
use axum::routing::post;
use axum::Router;
use dashmap::DashMap;
use reqwest::Client;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use tokio::net::TcpListener;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Shared DashMap for tests that need the proxy auth middleware.
// `set_virtual_keys` uses a global OnceLock — whichever test runs first wins.
// All proxy-auth tests share this one Arc<DashMap> so the middleware always
// looks at the same map that the tests populate.
// ---------------------------------------------------------------------------

static TEST_VK_MAP: OnceLock<Arc<DashMap<[u8; 32], admin::keys::VirtualKeyMeta>>> = OnceLock::new();
static TEST_HMAC_SECRET: OnceLock<Arc<Vec<u8>>> = OnceLock::new();

fn shared_vk_map() -> Arc<DashMap<[u8; 32], admin::keys::VirtualKeyMeta>> {
    TEST_VK_MAP
        .get_or_init(|| {
            let map = Arc::new(DashMap::new());
            anyllm_proxy::server::middleware::set_virtual_keys(map.clone());
            map
        })
        .clone()
}

fn shared_hmac_secret() -> Arc<Vec<u8>> {
    TEST_HMAC_SECRET
        .get_or_init(|| {
            // Use a fixed test secret so all tests agree on hash values.
            let secret = Arc::new(b"test-hmac-secret-for-integration".to_vec());
            anyllm_proxy::server::middleware::set_hmac_secret(secret.clone());
            secret
        })
        .clone()
}

/// Build a SharedState whose `virtual_keys` is the shared test map.
fn shared_state() -> admin::state::SharedState {
    let mut state = admin::state::SharedState::new_for_test();
    state.virtual_keys = shared_vk_map();
    state.hmac_secret = shared_hmac_secret();
    state
}

// ---------------------------------------------------------------------------
// Admin API CRUD tests (T038)
// ---------------------------------------------------------------------------

/// CSRF token used by unit-level tests (oneshot). Must match the cookie value below.
const TEST_CSRF_TOKEN: &str = "0000000000000000000000000000000000000000000000000000000000000001";
/// Cookie header value that satisfies the CSRF double-submit check for the token above.
const TEST_CSRF_COOKIE: &str =
    "csrf_token=0000000000000000000000000000000000000000000000000000000000000001";

fn test_admin_router() -> (Router, admin::state::SharedState) {
    // Raise admin rate limit so parallel tests from 127.0.0.1 don't starve each other.
    admin::routes::set_admin_rpm(10_000);
    let state = shared_state();
    // Pre-register the test CSRF token so the first mutating request passes.
    // Tests that make multiple mutations must call reinsert_csrf(&state) before
    // each additional mutation (one-time tokens are consumed on use).
    state
        .issued_csrf_tokens
        .insert(TEST_CSRF_TOKEN.to_string(), ());
    let token = Arc::new(zeroize::Zeroizing::new("test-admin-token".to_string()));
    let router = admin::routes::admin_router(state.clone(), token)
        // ConnectInfo extractor requires the service to be wrapped with
        // into_make_service_with_connect_info in production. In tests we use
        // MockConnectInfo so handlers can extract a fake peer address.
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
    (router, state)
}

/// Re-register the test CSRF token before a subsequent mutating request.
/// The CSRF middleware consumes tokens on first use; call this between mutations.
fn reinsert_csrf(state: &admin::state::SharedState) {
    state
        .issued_csrf_tokens
        .insert(TEST_CSRF_TOKEN.to_string(), ());
}

#[tokio::test]
async fn create_key_returns_201_with_raw_key() {
    let (app, _state) = test_admin_router();
    let req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "test key", "rpm_limit": 60})).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(body["key"].as_str().unwrap().starts_with("sk-vk"));
    assert!(body["id"].as_i64().is_some());
    assert_eq!(body["description"], "test key");
    assert_eq!(body["rpm_limit"], 60);
}

#[tokio::test]
async fn list_keys_returns_created_keys() {
    let (app, state) = test_admin_router();

    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "list-test"})).unwrap(),
        ))
        .unwrap();
    let _ = app.clone().oneshot(create_req).await.unwrap();
    // GET does not consume a CSRF token; no reinsert needed here.
    let _ = &state; // satisfy compiler: state used for reinsert if needed

    let list_req = Request::get("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(list_req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let keys = body["keys"].as_array().unwrap();
    assert!(!keys.is_empty());
}

#[tokio::test]
async fn revoke_key_removes_from_dashmap() {
    let (app, state) = test_admin_router();

    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "revoke-test"})).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(create_req).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let id = body["id"].as_i64().unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();

    let hash = admin::keys::hmac_hash_key(&raw_key, &state.hmac_secret);
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    assert!(state.virtual_keys.contains_key(&hash_bytes));

    reinsert_csrf(&state);
    let revoke_req = Request::delete(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(revoke_req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["status"], "revoked");
    assert!(!state.virtual_keys.contains_key(&hash_bytes));
}

#[tokio::test]
async fn revoke_nonexistent_key_returns_404() {
    let (app, _state) = test_admin_router();
    let req = Request::delete("/admin/api/keys/9999")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ---------------------------------------------------------------------------
// Update key (PUT /admin/api/keys/{id}) tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_key_returns_200_with_updated_fields() {
    let (app, state) = test_admin_router();

    // Create a key first.
    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "update-test", "rpm_limit": 10})).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let id = body["id"].as_i64().unwrap();

    reinsert_csrf(&state);
    // Update description and rpm_limit.
    let update_req = Request::put(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({
                "description": "updated-desc",
                "rpm_limit": 200,
                "allowed_models": ["gpt-4o", "claude-*"]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(update_req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["description"], "updated-desc");
    assert_eq!(body["rpm_limit"], 200);
    let models = body["allowed_models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0], "gpt-4o");
    assert_eq!(models[1], "claude-*");
}

#[tokio::test]
async fn update_nonexistent_key_returns_404() {
    let (app, _state) = test_admin_router();
    let req = Request::put("/admin/api/keys/99999")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "no-such-key"})).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn update_revoked_key_returns_404() {
    let (app, state) = test_admin_router();

    // Create then revoke.
    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "revoke-then-update"})).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(create_req).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let id = body["id"].as_i64().unwrap();

    reinsert_csrf(&state);
    let revoke_req = Request::delete(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(revoke_req).await.unwrap();
    assert_eq!(resp.status(), 200);

    reinsert_csrf(&state);
    // Update should fail with 404.
    let update_req = Request::put(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "should-fail"})).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(update_req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn update_key_refreshes_dashmap() {
    let (app, state) = test_admin_router();

    // Create key.
    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"description": "dashmap-update-test", "rpm_limit": 10}))
                .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    let id = body["id"].as_i64().unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();

    reinsert_csrf(&state);
    // Update rpm_limit via PUT.
    let update_req = Request::put(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({"rpm_limit": 500})).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(update_req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify DashMap entry was updated.
    let hash = admin::keys::hmac_hash_key(&raw_key, &state.hmac_secret);
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    let meta = state
        .virtual_keys
        .get(&hash_bytes)
        .expect("key should exist in DashMap");
    assert_eq!(meta.rpm_limit, Some(500));
}

// ---------------------------------------------------------------------------
// Helpers for proxy-level tests
// ---------------------------------------------------------------------------

fn openai_config_with_base(base_url: &str) -> Config {
    Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: anyllm_proxy::config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
    }
}

async fn spawn_mock_backend() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            axum::Json(json!({
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Spawn a proxy backed by the shared VK map so auth middleware can find virtual keys.
/// Includes a dummy /admin/api/test route behind auth to test RBAC.
async fn spawn_proxy_with_shared_vk(config: Config) -> String {
    let state = shared_state(); // must call before building app to ensure set_virtual_keys fires
    let multi = anyllm_proxy::config::MultiConfig::from_single_config(&config);
    let base_app = routes::app_multi_with_shared(multi, Some(state), None, None, None);

    // Add a test /admin/ route behind the same auth middleware so RBAC can be tested.
    let admin_test = Router::new()
        .route(
            "/admin/api/test",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .layer(axum::middleware::from_fn(
            anyllm_proxy::server::middleware::validate_auth,
        ));

    let app = base_app.merge(admin_test);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// Virtual key auth lifecycle (T038): create → use → revoke → rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn virtual_key_auth_and_revocation_lifecycle() {
    let mock = spawn_mock_backend().await;
    let proxy_url = spawn_proxy_with_shared_vk(openai_config_with_base(&mock)).await;

    // Admin server uses shared VK map so create/revoke affect the same DashMap
    // the middleware checks.
    let state = shared_state();
    let admin_app = admin::routes::admin_router(
        state,
        Arc::new(zeroize::Zeroizing::new("admin-token".to_string())),
    );
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = admin_listener.local_addr().unwrap().port();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    tokio::spawn(async move {
        axum::serve(
            admin_listener,
            admin_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });

    let client = Client::new();

    // 1. Create a virtual key (fetch fresh CSRF token for each mutation)
    let csrf = fetch_csrf(&client, &admin_url, admin_port).await;
    let resp = client
        .post(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token")
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("csrf_token={csrf}"))
        .json(&json!({"description": "lifecycle-test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();
    let key_id = body["id"].as_i64().unwrap();

    // 2. Use the virtual key to authenticate
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "virtual key should authenticate");

    // 3. Revoke the key
    let csrf = fetch_csrf(&client, &admin_url, admin_port).await;
    let resp = client
        .delete(format!("{admin_url}/admin/api/keys/{key_id}"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token")
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("csrf_token={csrf}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 4. Revoked key must be rejected
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "revoked key should be rejected");
}

// ---------------------------------------------------------------------------
// RPM rate limiting (T051): create key with rpm_limit:2, 3rd request → 429
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpm_limit_returns_429_after_exceeded() {
    let mock = spawn_mock_backend().await;
    let proxy_url = spawn_proxy_with_shared_vk(openai_config_with_base(&mock)).await;

    let state = shared_state();
    let admin_app = admin::routes::admin_router(
        state,
        Arc::new(zeroize::Zeroizing::new("admin-token2".to_string())),
    );
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = admin_listener.local_addr().unwrap().port();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    tokio::spawn(async move {
        axum::serve(
            admin_listener,
            admin_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });

    let client = Client::new();

    // Create a key with rpm_limit: 2
    let csrf = fetch_csrf(&client, &admin_url, admin_port).await;
    let resp = client
        .post(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token2")
        .header("x-csrf-token", &csrf)
        .header("cookie", format!("csrf_token={csrf}"))
        .json(&json!({"description": "rate-limit-test", "rpm_limit": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();

    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "Hi"}]
    });

    // First 2 requests should succeed
    for _ in 0..2 {
        let resp = client
            .post(format!("{proxy_url}/v1/messages"))
            .header("x-api-key", &raw_key)
            .json(&msg)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    // 3rd request must be rate-limited
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&msg)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    assert!(
        resp.headers().get("retry-after").is_some(),
        "429 must include retry-after header"
    );
}

// ---------------------------------------------------------------------------
// Budget enforcement tests (US5: T046)
// ---------------------------------------------------------------------------

/// Fetch a fresh server-issued CSRF token from the real admin server.
async fn fetch_csrf(client: &Client, admin_url: &str, admin_port: u16) -> String {
    let resp = client
        .get(format!("{admin_url}/admin/csrf-token"))
        .header("host", format!("localhost:{admin_port}"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    body["csrf_token"].as_str().unwrap().to_string()
}

/// Helper: create a key via admin API and return (raw_key, key_id).
async fn create_key_via_admin(
    admin_url: &str,
    admin_port: u16,
    admin_token: &str,
    body: serde_json::Value,
) -> (String, i64) {
    let client = Client::new();
    // Fetch a fresh server-issued CSRF token (one-time use).
    let csrf_resp = client
        .get(format!("{admin_url}/admin/csrf-token"))
        .header("host", format!("localhost:{admin_port}"))
        .send()
        .await
        .unwrap();
    let csrf_body: serde_json::Value = csrf_resp.json().await.unwrap();
    let csrf_token = csrf_body["csrf_token"].as_str().unwrap().to_string();
    let csrf_cookie = format!("csrf_token={csrf_token}");

    let resp = client
        .post(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", format!("Bearer {admin_token}"))
        .header("x-csrf-token", &csrf_token)
        .header("cookie", &csrf_cookie)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create key failed");
    let body: serde_json::Value = resp.json().await.unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();
    let key_id = body["id"].as_i64().unwrap();
    (raw_key, key_id)
}

/// Helper: spawn admin + proxy servers, returns (proxy_url, admin_url, admin_port).
async fn spawn_test_servers(admin_token: &str) -> (String, String, u16) {
    // Raise admin rate limit so parallel tests from 127.0.0.1 don't starve each other.
    admin::routes::set_admin_rpm(10_000);

    let mock = spawn_mock_backend().await;
    let proxy_url = spawn_proxy_with_shared_vk(openai_config_with_base(&mock)).await;

    let state = shared_state();
    let admin_app = admin::routes::admin_router(
        state,
        Arc::new(zeroize::Zeroizing::new(admin_token.to_string())),
    );
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = admin_listener.local_addr().unwrap().port();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    tokio::spawn(async move {
        axum::serve(
            admin_listener,
            admin_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });

    (proxy_url, admin_url, admin_port)
}

#[tokio::test]
async fn budget_exceeded_returns_429_with_budget_exceeded_type() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("budget-token1").await;

    // Create key with a tiny budget ($0.0001) and no duration (lifetime budget)
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "budget-token1",
        json!({"description": "budget-test", "max_budget_usd": 0.0001}),
    )
    .await;

    // Manually set the key's period_spend above the limit in the DashMap
    let vk_map = shared_vk_map();
    let hash = admin::keys::hmac_hash_key(&raw_key, &shared_hmac_secret());
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    if let Some(mut meta) = vk_map.get_mut(&hash_bytes) {
        meta.period_spend_usd = 1.0; // Way over the $0.0001 limit
    }

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429, "budget exceeded should return 429");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"]["type"], "budget_exceeded",
        "error type must be budget_exceeded, not rate_limit_exceeded"
    );
    assert!(
        body["error"]["budget_limit_usd"].as_f64().is_some(),
        "response must include budget_limit_usd"
    );
    assert!(
        body["error"]["period_spend_usd"].as_f64().is_some(),
        "response must include period_spend_usd"
    );
}

#[tokio::test]
async fn budget_not_exceeded_allows_request() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("budget-token2").await;

    // Create key with generous budget
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "budget-token2",
        json!({"description": "budget-ok-test", "spend_limit": 100.0, "budget_duration": "monthly"}),
    )
    .await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "under-budget key should succeed");
}

#[tokio::test]
async fn budget_resets_on_new_period() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("budget-token3").await;

    // Create key with daily budget
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "budget-token3",
        json!({"description": "budget-reset-test", "max_budget_usd": 0.0001, "budget_duration": "daily"}),
    )
    .await;

    // Manually set the key's spend above limit AND set period_start to yesterday
    let vk_map = shared_vk_map();
    let hash = admin::keys::hmac_hash_key(&raw_key, &shared_hmac_secret());
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    if let Some(mut meta) = vk_map.get_mut(&hash_bytes) {
        meta.period_spend_usd = 1.0; // Over budget
        meta.period_start = Some("2020-01-01T00:00:00Z".to_string()); // Long past
    }

    // The lazy reset should kick in and allow the request
    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "budget should reset for new period");
}

#[tokio::test]
async fn no_duration_budget_stays_blocked() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("budget-token4").await;

    // Create key with lifetime budget (no duration)
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "budget-token4",
        json!({"description": "lifetime-budget-test", "max_budget_usd": 0.0001}),
    )
    .await;

    // Set spend above limit; no duration means no reset
    let vk_map = shared_vk_map();
    let hash = admin::keys::hmac_hash_key(&raw_key, &shared_hmac_secret());
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    if let Some(mut meta) = vk_map.get_mut(&hash_bytes) {
        meta.period_spend_usd = 1.0;
    }

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429, "lifetime budget should stay blocked");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "budget_exceeded");
    // No budget_duration = lifetime (null in response)
    assert!(body["error"]["budget_duration"].is_null());
}

// ---------------------------------------------------------------------------
// RBAC tests (US6: T050)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn developer_key_succeeds_on_v1_messages() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("rbac-token1").await;

    // Create developer key (default role)
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "rbac-token1",
        json!({"description": "dev-key-test"}),
    )
    .await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "developer key should succeed on /v1/messages"
    );
}

#[tokio::test]
async fn developer_key_gets_403_on_admin() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("rbac-token2").await;

    // Create developer key explicitly
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "rbac-token2",
        json!({"description": "dev-admin-test", "role": "developer"}),
    )
    .await;

    // Access the test /admin/ route on the proxy (added by spawn_proxy_with_shared_vk).
    // The auth middleware checks RBAC before the route handler runs.
    let client = Client::new();
    let resp = client
        .get(format!("{proxy_url}/admin/api/test"))
        .header("x-api-key", &raw_key)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "developer key should get 403 on /admin/ path"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "permission_denied");
}

#[tokio::test]
async fn admin_key_succeeds_on_v1_messages() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("rbac-token3").await;

    // Create admin key
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "rbac-token3",
        json!({"description": "admin-key-test", "role": "admin"}),
    )
    .await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", &raw_key)
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "admin key should succeed on /v1/messages"
    );
}

#[tokio::test]
async fn admin_key_not_blocked_on_admin_path() {
    let (proxy_url, admin_url, admin_port) = spawn_test_servers("rbac-token4").await;

    // Create admin key
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "rbac-token4",
        json!({"description": "admin-path-test", "role": "admin"}),
    )
    .await;

    let client = Client::new();
    let resp = client
        .get(format!("{proxy_url}/admin/api/test"))
        .header("x-api-key", &raw_key)
        .send()
        .await
        .unwrap();
    // Admin key should NOT get 403. The test route returns 200.
    assert_eq!(
        resp.status(),
        200,
        "admin key should not get 403 on /admin/ path"
    );
}

#[tokio::test]
async fn new_key_defaults_to_developer_role() {
    let (_proxy_url, admin_url, admin_port) = spawn_test_servers("rbac-token5").await;

    // Create key with no explicit role
    let (raw_key, _key_id) = create_key_via_admin(
        &admin_url,
        admin_port,
        "rbac-token5",
        json!({"description": "default-role-test"}),
    )
    .await;

    // Check that the in-memory meta has developer role
    let vk_map = shared_vk_map();
    let hash = admin::keys::hmac_hash_key(&raw_key, &shared_hmac_secret());
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    let meta = vk_map.get(&hash_bytes).expect("key should exist in map");
    assert_eq!(
        meta.role,
        admin::keys::KeyRole::Developer,
        "new keys should default to developer role"
    );

    // Also check via list endpoint
    let client = Client::new();
    let resp = client
        .get(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer rbac-token5")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys = body["keys"].as_array().unwrap();
    // Find our key by description
    let our_key = keys
        .iter()
        .find(|k| k["description"] == "default-role-test")
        .expect("our key should appear in list");
    assert_eq!(our_key["role"], "developer");
}

// ---------------------------------------------------------------------------
// Model management validation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_model_rejects_unknown_backend() {
    use anyllm_proxy::config::model_router::ModelRouter;
    use std::collections::HashMap;
    use std::sync::RwLock;

    admin::routes::set_admin_rpm(10_000);
    let mut state = admin::state::SharedState::new_for_test();
    // Give it a model_router so the handler passes the "no model router active" guard.
    state.model_router = Some(Arc::new(RwLock::new(ModelRouter::new(HashMap::new()))));
    // backend_metrics is empty by default — any backend_name should be rejected.
    state
        .issued_csrf_tokens
        .insert(TEST_CSRF_TOKEN.to_string(), ());

    let token = Arc::new(zeroize::Zeroizing::new("test-admin-token".to_string()));
    let app = admin::routes::admin_router(state, token)
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let req = Request::post("/admin/api/models")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .header("x-csrf-token", TEST_CSRF_TOKEN)
        .header("cookie", TEST_CSRF_COOKIE)
        .body(Body::from(
            serde_json::to_string(&json!({
                "model_name": "my-model",
                "backend_name": "nonexistent",
                "actual_model": "gpt-4o"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("unknown backend"),
        "expected 'unknown backend' in error, got: {:?}",
        body
    );
}

// ---------------------------------------------------------------------------
// Pagination has_more field tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_requests_response_has_has_more_field() {
    let (app, _state) = test_admin_router();
    let req = Request::get("/admin/api/requests?limit=10&offset=0")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        body.get("has_more").is_some(),
        "response should include has_more field"
    );
    assert_eq!(body["has_more"], serde_json::Value::Bool(false));
}

#[tokio::test]
async fn get_audit_response_has_has_more_field() {
    let (app, _state) = test_admin_router();
    let req = Request::get("/admin/api/audit?limit=10&offset=0")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        body.get("has_more").is_some(),
        "response should include has_more field"
    );
    assert_eq!(body["has_more"], serde_json::Value::Bool(false));
}
