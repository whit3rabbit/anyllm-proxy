// Integration test for gateway-discovery explicit-pick routing: when the
// autorouter is enabled and a request's `model` is a real model a managed
// backend offers (selected by the user from /v1/models), it routes straight to
// that backend, bypassing autorouter tier signals. claude-* alias traffic still
// flows through the autorouter tiers.

use anyllm_proxy::admin::state::SharedState;
use anyllm_proxy::backend::BackendClient;
use anyllm_proxy::config::router_config::{RouterConfig, TierTarget};
use anyllm_proxy::config::{
    BackendAuth, BackendConfig, BackendKind, ModelMapping, MultiConfig, OpenAIApiFormat, TlsConfig,
};
use anyllm_proxy::server::routes;
use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Stub OpenAI chat-completions backend that always answers `content`.
async fn spawn_stub(content: &'static str) -> String {
    async fn handler(
        State(content): State<&'static str>,
        Json(_): Json<Value>,
    ) -> impl IntoResponse {
        Json(json!({
            "id": "chatcmpl-stub",
            "object": "chat.completion",
            "created": 1_700_000_000u64,
            "model": "stub",
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

fn openai_backend_config(base_url: String, provider_id: &str) -> BackendConfig {
    BackendConfig {
        kind: BackendKind::OpenAI,
        provider_id: Some(provider_id.to_string()),
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

fn tier(backend: &str, model: &str) -> TierTarget {
    TierTarget {
        backend_name: backend.to_string(),
        model: model.to_string(),
        enabled: true,
    }
}

async fn post_messages(proxy: &str, model: &str, thinking: bool) -> (u16, String) {
    let mut body = json!({
        "model": model,
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "hi"}]
    });
    if thinking {
        body["thinking"] = json!({"type": "enabled", "budget_tokens": 1024});
    }
    let resp = reqwest::Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.text().await.unwrap())
}

#[tokio::test]
async fn explicit_pick_beats_think_tier() {
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let default_url = spawn_stub("DEFAULT-WRONG").await;
    let think_url = spawn_stub("THINK-OK").await;
    let picked_url = spawn_stub("PICKED-OK").await;

    let shared = SharedState::new_for_test();
    // picked-be is a deepseek backend: its catalog offers "deepseek-chat", which
    // openai's catalog does not, so backend_for_real_model resolves unambiguously.
    let picked_row = anyllm_proxy::admin::db::ManagedBackendRow {
        id: "be-picked".to_string(),
        name: "picked-be".to_string(),
        provider_id: "deepseek".to_string(),
        api_key: Some("test-key".to_string()),
        api_base: Some(picked_url.clone()),
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
    let think_row = anyllm_proxy::admin::db::ManagedBackendRow {
        id: "be-think".to_string(),
        name: "think-be".to_string(),
        provider_id: "openai".to_string(),
        api_key: Some("test-key".to_string()),
        api_base: Some(think_url.clone()),
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
    {
        let mut map = shared.managed_backends.write().unwrap();
        map.insert(
            "picked-be".to_string(),
            (
                picked_row.clone(),
                BackendClient::from_backend_config(&openai_backend_config(picked_url, "deepseek")),
            ),
        );
        map.insert(
            "think-be".to_string(),
            (
                think_row.clone(),
                BackendClient::from_backend_config(&openai_backend_config(think_url, "openai")),
            ),
        );
    }
    // Enable the autorouter with a Think tier -> think-be. Default tier is left
    // unconfigured; it is irrelevant to both requests below.
    shared.runtime_config.write().unwrap().router = RouterConfig {
        enabled: true,
        context_threshold: 60_000,
        think: tier("think-be", "think-model"),
        ..Default::default()
    };

    // Default backend (the app's own) points at the "wrong" stub.
    let mut backends = indexmap::IndexMap::new();
    backends.insert(
        "default".to_string(),
        openai_backend_config(default_url, "openai"),
    );
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

    // 1. Real model pick with thinking on: explicit pick wins, routes to
    //    picked-be despite the Think tier matching.
    let (status, body) = post_messages(&proxy, "deepseek-chat", true).await;
    assert_eq!(status, 200);
    assert!(
        body.contains("PICKED-OK"),
        "explicit pick should route to picked-be; got: {body}"
    );
    assert!(
        !body.contains("THINK-OK") && !body.contains("DEFAULT-WRONG"),
        "think tier / default must not override an explicit pick; got: {body}"
    );

    // 2. claude-* alias with thinking on: explicit pick does not apply, the
    //    autorouter Think tier routes to think-be.
    let (status, body) = post_messages(&proxy, "claude-sonnet-4-5", true).await;
    assert_eq!(status, 200);
    assert!(
        body.contains("THINK-OK"),
        "claude-* alias with thinking should hit the think tier; got: {body}"
    );
}
