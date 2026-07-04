use anyllm_proxy::config::model_router::{Deployment, ModelRouter};
use anyllm_proxy::config::{
    BackendAuth, BackendConfig, BackendKind, ModelMapping, MultiConfig, OpenAIApiFormat, TlsConfig,
};
use anyllm_proxy::server::routes;
use axum::{body::Body, extract::State, response::IntoResponse, routing::post, Json, Router};
use bytes::Bytes;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Notify};
use tokio_stream::wrappers::ReceiverStream;

async fn spawn_hanging_openai_streaming_backend(release: Arc<Notify>) -> String {
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

async fn spawn_proxy(backend_base_url: String, deployment: Arc<Deployment>) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    let mut backends = IndexMap::new();
    backends.insert(
        "openai".to_string(),
        BackendConfig {
            kind: BackendKind::OpenAI,
            provider_id: None,
            api_key: "test-key".to_string(),
            base_url: backend_base_url,
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
        },
    );
    let config = MultiConfig {
        listen_port: 0,
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        forward_client_auth: false,
        default_backend: "openai".to_string(),
        backends,
        expose_degradation_warnings: false,
    };
    let mut model_routes = HashMap::new();
    model_routes.insert("virtual-stream".to_string(), vec![deployment]);
    let model_router = Arc::new(RwLock::new(ModelRouter::new(model_routes)));

    let app = routes::app_multi_with_shared(config, None, Some(model_router), None, None, None);
    spawn_app(app).await
}

async fn wait_for_in_flight(deployment: &Deployment, expected: u32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if deployment.in_flight_count() == expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "expected in-flight count {expected}, got {}",
            deployment.in_flight_count()
        )
    });
}

#[tokio::test]
async fn messages_streaming_keeps_deployment_in_flight_until_body_finishes() {
    let release = Arc::new(Notify::new());
    let backend = spawn_hanging_openai_streaming_backend(release.clone()).await;
    let deployment = Arc::new(Deployment::new(
        "openai".to_string(),
        "gpt-4o".to_string(),
        None,
        None,
    ));
    let proxy = spawn_proxy(backend, deployment.clone()).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .json(&json!({
            "model": "virtual-stream",
            "max_tokens": 32,
            "stream": true,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;

    assert_eq!(
        deployment.in_flight_count(),
        1,
        "deployment should remain in-flight while Messages stream body is open"
    );

    release.notify_waiters();
    let text = resp.text().await.unwrap();
    assert!(text.contains("event:"));
    wait_for_in_flight(&deployment, 0).await;
}

#[tokio::test]
async fn chat_completions_streaming_keeps_deployment_in_flight_until_body_finishes() {
    let release = Arc::new(Notify::new());
    let backend = spawn_hanging_openai_streaming_backend(release.clone()).await;
    let deployment = Arc::new(Deployment::new(
        "openai".to_string(),
        "gpt-4o".to_string(),
        None,
        None,
    ));
    let proxy = spawn_proxy(backend, deployment.clone()).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("authorization", "Bearer test")
        .json(&json!({
            "model": "virtual-stream",
            "max_tokens": 32,
            "stream": true,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;

    assert_eq!(
        deployment.in_flight_count(),
        1,
        "deployment should remain in-flight while Chat Completions stream body is open"
    );

    release.notify_waiters();
    let text = resp.text().await.unwrap();
    assert!(text.contains("data: [DONE]"));
    wait_for_in_flight(&deployment, 0).await;
}
