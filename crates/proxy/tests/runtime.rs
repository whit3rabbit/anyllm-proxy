use anyllm_proxy::config::{
    BackendAuth, BackendConfig, BackendKind, ModelMapping, MultiConfig, OpenAIApiFormat, TlsConfig,
};
use anyllm_proxy::runtime::{ChatCompletionError, ChatCompletionRuntime, ChatCompletionService};
use axum::{body::Body, extract::State, response::IntoResponse, routing::post, Json, Router};
use bytes::Bytes;
use futures::StreamExt;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Notify};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone)]
struct JsonBackendState {
    captured: Arc<Mutex<Option<Value>>>,
    response: Value,
}

async fn spawn_json_backend(captured: Arc<Mutex<Option<Value>>>, response: Value) -> String {
    async fn handler(
        State(state): State<JsonBackendState>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        *state.captured.lock().unwrap() = Some(body);
        let bytes = serde_json::to_vec(&state.response).unwrap();
        axum::response::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("x-ratelimit-limit-requests", "100")
            .header("x-ratelimit-remaining-requests", "99")
            .header("x-ratelimit-limit-tokens", "1000")
            .header("x-ratelimit-remaining-tokens", "900")
            .body(Body::from(bytes))
            .unwrap()
    }

    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(JsonBackendState { captured, response });
    spawn_app(app).await
}

async fn spawn_stream_backend(body: &'static str) -> String {
    async fn handler(State(body): State<&'static str>, Json(_): Json<Value>) -> impl IntoResponse {
        axum::response::Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .header("x-ratelimit-limit-requests", "100")
            .body(Body::from(body))
            .unwrap()
    }

    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(body);
    spawn_app(app).await
}

async fn spawn_hanging_stream_backend(release: Arc<Notify>) -> String {
    async fn handler(
        State(release): State<Arc<Notify>>,
        Json(_): Json<Value>,
    ) -> impl IntoResponse {
        let (tx, rx) = mpsc::channel::<Result<Bytes, std::convert::Infallible>>(4);
        tokio::spawn(async move {
            let first = concat!(
                "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",",
                "\"created\":1700000000,\"model\":\"gpt-4o\",",
                "\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},",
                "\"finish_reason\":null}]}\n\n"
            );
            let _ = tx.send(Ok(Bytes::from_static(first.as_bytes()))).await;
            release.notified().await;
            let final_events = concat!(
                "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",",
                "\"created\":1700000000,\"model\":\"gpt-4o\",",
                "\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,",
                "\"total_tokens\":5}}\n\n",
                "data: [DONE]\n\n"
            );
            let _ = tx
                .send(Ok(Bytes::from_static(final_events.as_bytes())))
                .await;
        });

        axum::response::Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .header("x-ratelimit-limit-requests", "100")
            .body(Body::from_stream(ReceiverStream::new(rx)))
            .unwrap()
    }

    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(release);
    spawn_app(app).await
}

async fn spawn_app(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn backend_config(kind: BackendKind, base_url: String) -> BackendConfig {
    BackendConfig {
        kind,
        provider_id: None,
        api_key: "test-key".to_string(),
        base_url,
        api_format: OpenAIApiFormat::Chat,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".to_string(),
            small_model: "gpt-4o-mini".to_string(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".to_string()),
        log_bodies: false,
        omit_stream_options: false,
        stream_timeout_secs: 900,
        bedrock_credentials: None,
    }
}

fn multi_config(backends: IndexMap<String, BackendConfig>, default_backend: &str) -> MultiConfig {
    MultiConfig {
        listen_port: 0,
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        forward_client_auth: false,
        default_backend: default_backend.to_string(),
        backends,
        expose_degradation_warnings: false,
    }
}

fn chat_response(model: &str) -> Value {
    json!({
        "id": "chatcmpl-runtime",
        "object": "chat.completion",
        "created": 1700000000u64,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 11,
            "completion_tokens": 7,
            "total_tokens": 18
        }
    })
}

#[tokio::test]
async fn runtime_preserves_openai_extra_fields_and_returns_usage_headers() {
    let captured = Arc::new(Mutex::new(None));
    let base_url = spawn_json_backend(captured.clone(), chat_response("gpt-4o")).await;

    let mut backends = IndexMap::new();
    backends.insert(
        "openai".to_string(),
        backend_config(BackendKind::OpenAI, base_url),
    );
    let runtime = ChatCompletionRuntime::from_multi_config(multi_config(backends, "openai"));

    let req = serde_json::from_value(json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32,
        "seed": 42,
        "top_k": 50,
        "min_p": 0.1,
        "repeat_penalty": 1.05,
        "chat_template_kwargs": {"enable_thinking": false}
    }))
    .unwrap();

    let result = runtime.complete(req).await.unwrap();

    assert_eq!(result.usage.unwrap().total_tokens, 18);
    assert_eq!(result.rate_limits.requests_limit.as_deref(), Some("100"));
    assert_eq!(result.rate_limits.tokens_remaining.as_deref(), Some("900"));
    assert_eq!(result.metadata.selected_backend, "openai");
    assert_eq!(result.metadata.mapped_model, "gpt-4o");
    assert!(!result.metadata.used_responses_api);

    let sent = captured.lock().unwrap().clone().unwrap();
    assert_eq!(sent["seed"], 42);
    assert_eq!(sent["top_k"], 50);
    assert_eq!(sent["min_p"], 0.1);
    assert_eq!(sent["repeat_penalty"], 1.05);
    assert_eq!(sent["chat_template_kwargs"]["enable_thinking"], false);
}

#[tokio::test]
async fn runtime_model_router_selects_backend_and_mapped_model() {
    let captured_a = Arc::new(Mutex::new(None));
    let captured_b = Arc::new(Mutex::new(None));
    let base_a = spawn_json_backend(captured_a.clone(), chat_response("unused")).await;
    let base_b = spawn_json_backend(captured_b.clone(), chat_response("actual-model")).await;

    let mut backends = IndexMap::new();
    backends.insert("a".to_string(), backend_config(BackendKind::OpenAI, base_a));
    backends.insert("b".to_string(), backend_config(BackendKind::OpenAI, base_b));

    let deployment = Arc::new(anyllm_proxy::config::model_router::Deployment::new(
        "b".to_string(),
        "actual-model".to_string(),
        None,
        None,
    ));
    let mut routes = HashMap::new();
    routes.insert("virtual-model".to_string(), vec![deployment]);
    let router = Arc::new(RwLock::new(
        anyllm_proxy::config::model_router::ModelRouter::new(routes),
    ));

    let runtime = ChatCompletionRuntime::from_multi_config_with_model_router(
        multi_config(backends, "a"),
        Some(router),
    );
    let req = serde_json::from_value(json!({
        "model": "virtual-model",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32
    }))
    .unwrap();

    let result = runtime.complete(req).await.unwrap();

    assert_eq!(result.metadata.selected_backend, "b");
    assert_eq!(result.metadata.requested_model, "virtual-model");
    assert_eq!(result.metadata.mapped_model, "actual-model");
    assert!(captured_a.lock().unwrap().is_none());
    assert_eq!(
        captured_b.lock().unwrap().clone().unwrap()["model"],
        "actual-model"
    );
}

#[tokio::test]
async fn runtime_model_router_rejects_unconfigured_model() {
    let captured_a = Arc::new(Mutex::new(None));
    let base_a = spawn_json_backend(captured_a.clone(), chat_response("unused")).await;

    let mut backends = IndexMap::new();
    backends.insert("a".to_string(), backend_config(BackendKind::OpenAI, base_a));

    let deployment = Arc::new(anyllm_proxy::config::model_router::Deployment::new(
        "a".to_string(),
        "actual-model".to_string(),
        None,
        None,
    ));
    let mut routes = HashMap::new();
    routes.insert("virtual-model".to_string(), vec![deployment]);
    let router = Arc::new(RwLock::new(
        anyllm_proxy::config::model_router::ModelRouter::new(routes),
    ));

    let runtime = ChatCompletionRuntime::from_multi_config_with_model_router(
        multi_config(backends, "a"),
        Some(router),
    );
    let req = serde_json::from_value(json!({
        "model": "unconfigured-model",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32
    }))
    .unwrap();

    let err = runtime.complete(req).await.unwrap_err();
    assert!(matches!(err, ChatCompletionError::InvalidRequest(_)));
    assert!(captured_a.lock().unwrap().is_none());
}

#[tokio::test]
async fn runtime_returns_typed_error_for_unsupported_backend() {
    let mut backends = IndexMap::new();
    backends.insert(
        "anthropic".to_string(),
        backend_config(
            BackendKind::Anthropic,
            "https://api.anthropic.com".to_string(),
        ),
    );
    let runtime = ChatCompletionRuntime::from_multi_config(multi_config(backends, "anthropic"));
    let req = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32
    }))
    .unwrap();

    let err = runtime.complete(req).await.unwrap_err();

    match err {
        ChatCompletionError::UnsupportedBackend {
            backend_name,
            backend_kind,
        } => {
            assert_eq!(backend_name, "anthropic");
            assert_eq!(backend_kind, BackendKind::Anthropic);
        }
        other => panic!("expected UnsupportedBackend, got {other:?}"),
    }
}

#[tokio::test]
async fn runtime_streaming_emits_chunks_and_usage() {
    let sse = concat!(
        "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
        "data: [DONE]\n\n"
    );
    let base_url = spawn_stream_backend(sse).await;

    let mut backends = IndexMap::new();
    backends.insert(
        "openai".to_string(),
        backend_config(BackendKind::OpenAI, base_url),
    );
    let runtime = ChatCompletionRuntime::from_multi_config(multi_config(backends, "openai"));
    let req = serde_json::from_value(json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32,
        "stream": true
    }))
    .unwrap();

    let result = runtime.complete_stream(req).await.unwrap();
    assert_eq!(result.rate_limits.requests_limit.as_deref(), Some("100"));

    let chunks: Vec<_> = result
        .chunks
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect();

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("hi"));
    assert_eq!(chunks[1].usage.as_ref().unwrap().total_tokens, 5);
}

#[tokio::test]
async fn runtime_streaming_keeps_deployment_in_flight_until_body_finishes() {
    let release = Arc::new(Notify::new());
    let base_url = spawn_hanging_stream_backend(release.clone()).await;

    let mut backends = IndexMap::new();
    backends.insert(
        "openai".to_string(),
        backend_config(BackendKind::OpenAI, base_url),
    );
    let deployment = Arc::new(anyllm_proxy::config::model_router::Deployment::new(
        "openai".to_string(),
        "gpt-4o".to_string(),
        None,
        None,
    ));
    let mut routes = HashMap::new();
    routes.insert("virtual-stream".to_string(), vec![deployment.clone()]);
    let router = Arc::new(RwLock::new(
        anyllm_proxy::config::model_router::ModelRouter::new(routes),
    ));
    let runtime = ChatCompletionRuntime::from_multi_config_with_model_router(
        multi_config(backends, "openai"),
        Some(router),
    );
    let req = serde_json::from_value(json!({
        "model": "virtual-stream",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 32,
        "stream": true
    }))
    .unwrap();

    let result = runtime.complete_stream(req).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;

    assert_eq!(
        deployment.in_flight_count(),
        1,
        "deployment should remain in-flight while the streaming body is open"
    );

    release.notify_waiters();
    let chunks: Vec<_> = result
        .chunks
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect();

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[1].usage.as_ref().unwrap().total_tokens, 5);
    assert_eq!(deployment.in_flight_count(), 0);
    assert!(deployment.latency_ms() > 0);
}
