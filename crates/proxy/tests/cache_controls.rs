use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use axum::{
    body::Body,
    http::{header, Response},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::net::TcpListener;

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
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    }
}

async fn spawn_proxy(config: Config) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_counting_chat_backend() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/v1/chat/completions",
        post({
            let hits = hits.clone();
            move |Json(body): Json<Value>| {
                let hits = hits.clone();
                async move {
                    let hit = hits.fetch_add(1, Ordering::SeqCst) + 1;
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        let sse = concat!(
                            "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl-stream\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                            "data: [DONE]\n\n",
                        );
                        return Response::builder()
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(sse))
                            .unwrap();
                    }

                    Json(json!({
                        "id": format!("chatcmpl-cache-{hit}"),
                        "object": "chat.completion",
                        "created": 1700000000,
                        "model": "gpt-4o",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": format!("hit {hit}")},
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 2,
                            "total_tokens": 6
                        }
                    }))
                    .into_response()
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), hits)
}

fn base_request() -> Value {
    json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 100
    })
}

async fn post_chat(client: &Client, proxy_url: &str, body: Value) -> reqwest::Response {
    client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("x-api-key", "client-key")
        .json(&body)
        .send()
        .await
        .unwrap()
}

fn cache_header(response: &reqwest::Response) -> &str {
    response
        .headers()
        .get("x-anyllm-cache")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

#[tokio::test]
async fn chat_completions_identical_request_misses_then_hits() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();

    let first = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let second = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "hit");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chat_completions_no_cache_skips_lookup_but_stores() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();

    let first = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let mut refresh = base_request();
    refresh["cache"] = json!({"no-cache": true});
    let second = post_chat(&client, &proxy_url, refresh).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "bypass");
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let third = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(third.status(), 200);
    assert_eq!(cache_header(&third), "hit");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn chat_completions_no_store_does_not_populate_on_miss() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();

    let mut no_store = base_request();
    no_store["cache"] = json!({"no-store": true});
    let first = post_chat(&client, &proxy_url, no_store).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let second = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let third = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(third.status(), 200);
    assert_eq!(cache_header(&third), "hit");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn chat_completions_cache_ttl_does_not_fragment_key() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();

    let mut ttl_60 = base_request();
    ttl_60["cache"] = json!({"ttl": 60});
    let first = post_chat(&client, &proxy_url, ttl_60).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let mut ttl_3600 = base_request();
    ttl_3600["cache"] = json!({"ttl": 3600});
    let second = post_chat(&client, &proxy_url, ttl_3600).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "hit");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cached_multi_tool_request_does_not_mask_parallel_tool_calls_variant() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();
    let tools = json!([
        {
            "type": "function",
            "function": {
                "name": "lookup_city",
                "description": "lookup city",
                "parameters": {"type": "object", "properties": {}}
            }
        },
        {
            "type": "function",
            "function": {
                "name": "lookup_weather",
                "description": "lookup weather",
                "parameters": {"type": "object", "properties": {}}
            }
        }
    ]);

    let mut valid = base_request();
    valid["tools"] = tools.clone();
    let first = post_chat(&client, &proxy_url, valid).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let mut variant = base_request();
    variant["tools"] = tools;
    variant["parallel_tool_calls"] = json!(false);
    let second = post_chat(&client, &proxy_url, variant).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn streaming_request_bypasses_and_does_not_populate_cache() {
    let (backend_url, hits) = spawn_counting_chat_backend().await;
    let proxy_url = spawn_proxy(openai_config_with_base(&backend_url)).await;
    let client = Client::new();

    let mut stream = base_request();
    stream["stream"] = json!(true);
    let first = post_chat(&client, &proxy_url, stream).await;
    assert_eq!(first.status(), 200);
    assert_eq!(cache_header(&first), "bypass");
    let _ = first.text().await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let second = post_chat(&client, &proxy_url, base_request()).await;
    assert_eq!(second.status(), 200);
    assert_eq!(cache_header(&second), "miss");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}
