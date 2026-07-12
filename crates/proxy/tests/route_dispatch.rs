// Integration test for admin-DB route dispatch: a request whose `model` is
// served by a route must be dispatched to that route's managed backend, not the
// default backend. Exercises the real HTTP path through
// `AppState::resolve_model` -> `RouteRouter` -> managed_backends client.

use anyllm_proxy::admin::state::SharedState;
use anyllm_proxy::backend::BackendClient;
use anyllm_proxy::config::route_router::RouteRouter;
use anyllm_proxy::config::{
    BackendAuth, BackendConfig, BackendKind, ModelMapping, MultiConfig, OpenAIApiFormat, TlsConfig,
};
use anyllm_proxy::server::routes;
use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;

/// Spawn a stub OpenAI chat-completions backend that always answers with `content`.
async fn spawn_stub(content: &'static str) -> String {
    async fn handler(
        State(content): State<&'static str>,
        Json(_): Json<Value>,
    ) -> impl IntoResponse {
        Json(json!({
            "id": "chatcmpl-stub",
            "object": "chat.completion",
            "created": 1_700_000_000u64,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }))
    }
    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(content);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn openai_backend_config(base_url: String) -> BackendConfig {
    BackendConfig {
        kind: BackendKind::OpenAI,
        provider_id: Some("openai".to_string()),
        api_key: "test-key".to_string(),
        base_url,
        api_format: OpenAIApiFormat::Chat,
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".to_string()),
        log_bodies: false,
        omit_stream_options: false,
        stream_timeout_secs: 900,
        bedrock_credentials: None,
        allow_local_ssrf: true,
    }
}

#[tokio::test]
async fn route_dispatches_to_managed_backend_over_default() {
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let default_url = spawn_stub("DEFAULT-WRONG").await;
    let routed_url = spawn_stub("ROUTED-OK").await;

    // Build shared state with a route that serves "routed-model" via a managed backend.
    let shared = SharedState::new_for_test();
    let routed_bc = openai_backend_config(routed_url.clone());
    let row = anyllm_proxy::admin::db::ManagedBackendRow {
        id: "be-routed".to_string(),
        name: "routed-be".to_string(),
        provider_id: "openai".to_string(),
        api_key: Some("test-key".to_string()),
        api_base: Some(routed_url.clone()),
        deployment: None,
        api_version: None,
        project: None,
        region: None,
        aws_access_key_id: None,
        aws_secret_access_key: None,
        aws_session_token: None,
        rpm: None,
        tpm: None,
        enabled: true,
        created_at: "t".to_string(),
        updated_at: "t".to_string(),
    };
    let route = anyllm_proxy::admin::db::RouteRow {
        id: "rt-1".to_string(),
        name: "primary".to_string(),
        description: None,
        strategy: "failover".to_string(),
        rpm: None,
        tpm: None,
        budget_usd: None,
        enabled: true,
        guardrail_mode: None,
        pxpipe_compress: None,
        pxpipe_models: None,
        redact_secrets: None,
        position: 0,
        created_at: "t".to_string(),
        updated_at: "t".to_string(),
    };

    let route_router = {
        let conn = shared.db.lock().unwrap();
        anyllm_proxy::admin::db::insert_managed_backend(&conn, &row).unwrap();
        anyllm_proxy::admin::db::insert_route(&conn, &route).unwrap();
        anyllm_proxy::admin::db::add_route_provider(
            &conn,
            "rt-1",
            "be-routed",
            &["routed-model".to_string()],
            0,
            true,
        )
        .unwrap();
        RouteRouter::build_from_db(&conn).unwrap()
    };

    let mut shared = shared;
    shared.route_router = Some(Arc::new(RwLock::new(route_router)));
    {
        let mut map = shared.managed_backends.write().unwrap();
        map.insert(
            "routed-be".to_string(),
            (row.clone(), BackendClient::from_backend_config(&routed_bc)),
        );
    }

    // Default backend points at the "wrong" stub; routing must override it.
    let mut backends = IndexMap::new();
    backends.insert("default".to_string(), openai_backend_config(default_url));
    let config = MultiConfig {
        listen_port: 0,
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        forward_client_auth: false,
        default_backend: "default".to_string(),
        backends,
        expose_degradation_warnings: false,
    };

    let app = routes::app_multi_with_shared(config, Some(shared), None, None, None, None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let proxy = format!("http://{addr}");

    let resp = reqwest::Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "routed-model",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("ROUTED-OK"),
        "route should dispatch to the managed backend; got: {body}"
    );
    assert!(
        !body.contains("DEFAULT-WRONG"),
        "route must override the default backend; got: {body}"
    );
}
