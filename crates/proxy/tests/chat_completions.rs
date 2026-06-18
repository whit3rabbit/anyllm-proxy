// Integration tests for POST /v1/chat/completions (OpenAI-format input).

use anyllm_proxy::config::{self, BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat};
use anyllm_proxy::server::routes;
use anyllm_proxy::tools::execution::ToolEngineState;
use anyllm_proxy::tools::{
    LoopConfig, PolicyAction, PolicyRule, Tool, ToolExecutionPolicy, ToolRegistry,
};
use axum::{routing::post, Router};
use reqwest::Client;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
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

fn openai_config_with_degradation(base_url: &str) -> Config {
    Config {
        expose_degradation_warnings: true,
        ..openai_config_with_base(base_url)
    }
}

fn anthropic_config_with_base(base_url: &str) -> Config {
    Config {
        backend: BackendKind::Anthropic,
        openai_api_key: "anthropic-backend-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("anthropic-backend-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: true,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    }
}

/// Mock backend that returns a fixed OpenAI Chat Completions response.
async fn spawn_mock_chat_backend() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            axum::Json(json!({
                "id": "chatcmpl-mock123",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello from mock!"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_mock_anthropic_backend(
    captured_body: Arc<Mutex<Option<serde_json::Value>>>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
) -> String {
    let app = Router::new().route(
        "/v1/messages",
        post({
            let captured_body = captured_body.clone();
            let captured_headers = captured_headers.clone();
            move |headers: axum::http::HeaderMap,
                  axum::Json(body): axum::Json<serde_json::Value>| {
                let captured_body = captured_body.clone();
                let captured_headers = captured_headers.clone();
                async move {
                    *captured_body.lock().unwrap() = Some(body.clone());
                    let headers_vec = headers
                        .iter()
                        .filter_map(|(name, value)| {
                            value
                                .to_str()
                                .ok()
                                .map(|value| (name.as_str().to_string(), value.to_string()))
                        })
                        .collect();
                    *captured_headers.lock().unwrap() = headers_vec;

                    axum::Json(json!({
                        "id": "msg_mock123",
                        "type": "message",
                        "role": "assistant",
                        "model": body["model"].as_str().unwrap_or("claude-sonnet-4-6"),
                        "content": [{"type": "text", "text": "Hello from Anthropic mock!"}],
                        "stop_reason": "end_turn",
                        "stop_sequence": null,
                        "usage": {"input_tokens": 12, "output_tokens": 6}
                    }))
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_mock_anthropic_server_tool_backend() -> String {
    let app = Router::new().route(
        "/v1/messages",
        post(|| async {
            axum::Json(json!({
                "id": "msg_server_tool",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-6",
                "content": [{
                    "type": "server_tool_use",
                    "id": "srv_1",
                    "name": "web_search",
                    "input": {"query": "rust"}
                }],
                "stop_reason": "tool_use",
                "stop_sequence": null,
                "usage": {"input_tokens": 12, "output_tokens": 6}
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_mock_anthropic_stream_backend() -> String {
    let app = Router::new().route(
        "/v1/messages",
        post(|| async {
            let body = concat!(
                "event: message_start\n",
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-6\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"stream hello\"}}\n\n",
                "event: content_block_stop\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":4}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            (
                [("content-type", "text/event-stream")],
                body.to_string(),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_mock_anthropic_server_tool_stream_backend() -> String {
    let app = Router::new().route(
        "/v1/messages",
        post(|| async {
            let body = concat!(
                "event: message_start\n",
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream_tool\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-6\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srv_1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"rust\\\"}\"}}\n\n",
                "event: content_block_stop\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":4}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            ([("content-type", "text/event-stream")], body.to_string())
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_proxy(config: Config) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_proxy_with_tool_engine(config: Config, tool_engine: Arc<ToolEngineState>) -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let multi = anyllm_proxy::config::MultiConfig::from_single_config(&config);
    let app = routes::app_multi_with_shared(multi, None, None, Some(tool_engine), None, None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct CountingTool {
    executions: Arc<AtomicUsize>,
}

impl Tool for CountingTool {
    fn name(&self) -> &str {
        "server_tool"
    }

    fn description(&self) -> &str {
        "Test-only server tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    fn execute<'a>(
        &'a self,
        _input: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>> {
        let executions = self.executions.clone();
        Box::pin(async move {
            executions.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        })
    }
}

fn tool_engine_with_counting_tool(executions: Arc<AtomicUsize>) -> Arc<ToolEngineState> {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(CountingTool { executions }));
    Arc::new(ToolEngineState {
        registry: Arc::new(registry),
        policy: Arc::new(ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "server_tool".to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        }),
        loop_config: LoopConfig::default(),
        mcp_manager: None,
    })
}

async fn spawn_tool_call_chat_backend(hits: Arc<AtomicUsize>) -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post({
            let hits = hits.clone();
            move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!({
                        "id": "chatcmpl-tool-call",
                        "object": "chat.completion",
                        "created": 1700000000,
                        "model": "gpt-4o",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [{
                                    "id": "call_server_tool",
                                    "type": "function",
                                    "function": {
                                        "name": "server_tool",
                                        "arguments": "{\"value\":\"client-controlled\"}"
                                    }
                                }]
                            },
                            "finish_reason": "tool_calls"
                        }],
                        "usage": {
                            "prompt_tokens": 10,
                            "completion_tokens": 5,
                            "total_tokens": 15
                        }
                    }))
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn chat_completions_non_streaming() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["id"].as_str().unwrap().starts_with("chatcmpl-"));
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");
    assert!(body["choices"][0]["message"]["content"].as_str().is_some());
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert!(body["usage"]["prompt_tokens"].as_u64().is_some());
}

#[tokio::test]
async fn anthropic_chat_completions_maps_server_tool_use() {
    let mock = spawn_mock_anthropic_server_tool_backend().await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Search"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "web_search"
    );
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"query\":\"rust\"}"
    );
}

#[tokio::test]
async fn anthropic_chat_completions_defaults_and_extensions() {
    let captured_body = Arc::new(Mutex::new(None));
    let captured_headers = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(captured_body.clone(), captured_headers.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("authorization", "Bearer caller-secret")
        .header("anthropic-beta", "test-beta")
        .header("x-claude-code-session-id", "session-123")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Hello"}],
            "reasoning_effort": "medium",
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "reply",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {"answer": {"type": "string"}},
                        "required": ["answer"],
                        "additionalProperties": false
                    }
                }
            },
            "tools": [{
                "type": "function",
                "function": {
                    "name": "client_tool",
                    "description": "client-side tool",
                    "parameters": {"type": "object"}
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().get("x-anyllm-degradation").is_none(),
        "json_schema response_format should be handled by Anthropic output_config"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Hello from Anthropic mock!"
    );

    let upstream = captured_body.lock().unwrap().clone().unwrap();
    assert_eq!(upstream["model"], "claude-sonnet-4-6");
    assert_eq!(upstream["max_tokens"], 6144);
    assert_eq!(upstream["thinking"]["type"], "enabled");
    assert_eq!(upstream["thinking"]["budget_tokens"], 2048);
    assert!(upstream["output_config"].get("effort").is_none());
    assert_eq!(
        upstream["output_config"]["format"]["schema"]["properties"]["answer"]["type"],
        "string"
    );
    assert_eq!(upstream["tools"][0]["name"], "client_tool");

    let headers = captured_headers.lock().unwrap().clone();
    assert!(headers
        .iter()
        .any(|(name, value)| name == "anthropic-beta" && value == "test-beta"));
    assert!(headers
        .iter()
        .any(|(name, value)| name == "x-claude-code-session-id" && value == "session-123"));
    assert!(headers
        .iter()
        .any(|(name, value)| name == "x-api-key" && value == "anthropic-backend-key"));
    assert!(
        !headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer caller-secret"),
        "caller Authorization header must not be forwarded upstream"
    );
}

#[tokio::test]
async fn anthropic_chat_completions_streaming_translates_to_openai_sse() {
    let mock = spawn_mock_anthropic_stream_backend().await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100,
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("\"object\":\"chat.completion.chunk\""),
        "{text}"
    );
    assert!(text.contains("stream hello"), "{text}");
    assert!(text.contains("\"prompt_tokens\":9"), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");
}

#[tokio::test]
async fn anthropic_chat_completions_streaming_maps_server_tool_use() {
    let mock = spawn_mock_anthropic_server_tool_stream_backend().await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Search"}],
            "max_tokens": 100,
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.contains("\"tool_calls\""), "{text}");
    assert!(text.contains("\"name\":\"web_search\""), "{text}");
    assert!(text.contains("{\\\"query\\\":\\\"rust\\\"}"), "{text}");
    assert!(text.contains("\"finish_reason\":\"tool_calls\""), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");
}

#[tokio::test]
async fn chat_completions_does_not_execute_client_advertised_server_tool_name() {
    let backend_hits = Arc::new(AtomicUsize::new(0));
    let tool_executions = Arc::new(AtomicUsize::new(0));
    let mock = spawn_tool_call_chat_backend(backend_hits.clone()).await;
    let tool_engine = tool_engine_with_counting_tool(tool_executions.clone());
    let proxy = spawn_proxy_with_tool_engine(openai_config_with_base(&mock), tool_engine).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "call the server_tool"}],
            "max_tokens": 100,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "server_tool",
                    "description": "client-controlled schema using a server tool name",
                    "parameters": {"type": "object"}
                }
            }],
            "tool_choice": {
                "type": "function",
                "function": {"name": "server_tool"}
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(backend_hits.load(Ordering::SeqCst), 1);
    assert_eq!(tool_executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn chat_completions_missing_max_tokens_returns_400() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn chat_completions_empty_messages_returns_400() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

// When expose_degradation_warnings is true, lossy fields must appear in the header.
#[tokio::test]
async fn chat_completions_degradation_header_on_lossy_fields() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_degradation(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100,
            "presence_penalty": 0.5,
            "frequency_penalty": 0.3
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let degradation = resp
        .headers()
        .get("x-anyllm-degradation")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        degradation.contains("presence_penalty"),
        "expected presence_penalty in degradation header, got: {degradation}"
    );
    assert!(
        degradation.contains("frequency_penalty"),
        "expected frequency_penalty in degradation header, got: {degradation}"
    );
}

// When expose_degradation_warnings is false (default), the header must not be set
// even when lossy fields are present.
#[tokio::test]
async fn chat_completions_degradation_header_suppressed_when_disabled() {
    let mock = spawn_mock_chat_backend().await;
    // openai_config_with_base has expose_degradation_warnings: false
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100,
            "presence_penalty": 0.5,
            "frequency_penalty": 0.3
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().get("x-anyllm-degradation").is_none(),
        "x-anyllm-degradation must not be present when expose_degradation_warnings is false"
    );
}

#[tokio::test]
async fn chat_completions_with_system_message() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
}

#[tokio::test]
async fn chat_completions_returns_openai_error_format() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let client = Client::new();
    // Send completely invalid JSON
    let resp = client
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Should have OpenAI error format (error.type, error.message)
    assert!(body["error"]["type"].is_string());
    assert!(body["error"]["message"].is_string());
}
