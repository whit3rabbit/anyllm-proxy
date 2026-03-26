// Integration tests for virtual key admin API (T038) and rate limiting (T051).

use anyllm_proxy::admin;
use anyllm_proxy::config::{BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::body::Body;
use axum::http::Request;
use axum::routing::post;
use axum::Router;
use dashmap::DashMap;
use reqwest::Client;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tokio::net::TcpListener;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Shared DashMap for tests that need the proxy auth middleware.
// `set_virtual_keys` uses a global OnceLock — whichever test runs first wins.
// All proxy-auth tests share this one Arc<DashMap> so the middleware always
// looks at the same map that the tests populate.
// ---------------------------------------------------------------------------

static TEST_VK_MAP: OnceLock<Arc<DashMap<[u8; 32], admin::keys::VirtualKeyMeta>>> =
    OnceLock::new();

fn shared_vk_map() -> Arc<DashMap<[u8; 32], admin::keys::VirtualKeyMeta>> {
    TEST_VK_MAP
        .get_or_init(|| {
            let map = Arc::new(DashMap::new());
            anyllm_proxy::server::middleware::set_virtual_keys(map.clone());
            map
        })
        .clone()
}

/// Build a SharedState whose `virtual_keys` is the shared test map.
fn shared_state() -> admin::state::SharedState {
    let mut state = admin::state::SharedState::new_for_test();
    state.virtual_keys = shared_vk_map();
    state
}

// ---------------------------------------------------------------------------
// Admin API CRUD tests (T038)
// ---------------------------------------------------------------------------

fn test_admin_router() -> (Router, admin::state::SharedState) {
    let state = shared_state();
    let token = Arc::new("test-admin-token".to_string());
    let router = admin::routes::admin_router(state.clone(), token);
    (router, state)
}

#[tokio::test]
async fn create_key_returns_201_with_raw_key() {
    let (app, _state) = test_admin_router();
    let req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({"description": "test key", "rpm_limit": 60})).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap())
            .unwrap();
    assert!(body["key"].as_str().unwrap().starts_with("sk-vk"));
    assert!(body["id"].as_i64().is_some());
    assert_eq!(body["description"], "test key");
    assert_eq!(body["rpm_limit"], 60);
}

#[tokio::test]
async fn list_keys_returns_created_keys() {
    let (app, _state) = test_admin_router();

    let create_req = Request::post("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&json!({"description": "list-test"})).unwrap(),
        ))
        .unwrap();
    let _ = app.clone().oneshot(create_req).await.unwrap();

    let list_req = Request::get("/admin/api/keys")
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(list_req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap())
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
        .body(Body::from(
            serde_json::to_string(&json!({"description": "revoke-test"})).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(create_req).await.unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap())
            .unwrap();
    let id = body["id"].as_i64().unwrap();
    let raw_key = body["key"].as_str().unwrap().to_string();

    let hash = admin::keys::hash_key(&raw_key);
    let hash_bytes = admin::keys::hash_from_hex(&hash).unwrap();
    assert!(state.virtual_keys.contains_key(&hash_bytes));

    let revoke_req = Request::delete(format!("/admin/api/keys/{id}"))
        .header("host", "localhost:9090")
        .header("authorization", "Bearer test-admin-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(revoke_req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap())
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
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
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
async fn spawn_proxy_with_shared_vk(config: Config) -> String {
    let state = shared_state(); // must call before building app to ensure set_virtual_keys fires
    let multi = anyllm_proxy::config::MultiConfig::from_single_config(&config);
    let app = routes::app_multi_with_shared(multi, Some(state));
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
    let admin_app = admin::routes::admin_router(state, Arc::new("admin-token".to_string()));
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = admin_listener.local_addr().unwrap().port();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    tokio::spawn(async move { axum::serve(admin_listener, admin_app).await.unwrap() });

    let client = Client::new();

    // 1. Create a virtual key
    let resp = client
        .post(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token")
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
    let resp = client
        .delete(format!("{admin_url}/admin/api/keys/{key_id}"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token")
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
    let admin_app = admin::routes::admin_router(state, Arc::new("admin-token2".to_string()));
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = admin_listener.local_addr().unwrap().port();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    tokio::spawn(async move { axum::serve(admin_listener, admin_app).await.unwrap() });

    let client = Client::new();

    // Create a key with rpm_limit: 2
    let resp = client
        .post(format!("{admin_url}/admin/api/keys"))
        .header("host", format!("localhost:{admin_port}"))
        .header("authorization", "Bearer admin-token2")
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
