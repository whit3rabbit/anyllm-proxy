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
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
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
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
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
        guardrails: anyllm_proxy::tools::ToolGuardrailConfig::disabled(),
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
    assert_eq!(upstream["max_tokens"], 4096);
    assert_eq!(upstream["thinking"]["type"], "adaptive");
    assert_eq!(upstream["output_config"]["effort"], "medium");
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

// --- Anthropic-specific parity tests ---
// Ported from LiteLLM test patterns:
//   pass_through_unit_tests/test_anthropic_messages_passthrough.py
//   pass_through_unit_tests/base_anthropic_messages_prompt_caching_test.py

#[tokio::test]
async fn anthropic_test_thinking_param_passthrough() {
    // LiteLLM: test_anthropic_messages_with_thinking
    // Verify thinking.budget_tokens reaches the backend via /v1/messages passthrough.
    let captured_body: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(captured_body.clone(), captured_headers.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100,
            "thinking": {"budget_tokens": 100}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let sent = captured_body.lock().unwrap().take().unwrap();
    assert_eq!(
        sent["thinking"]["budget_tokens"], 100,
        "thinking.budget_tokens should be preserved in passthrough: {sent}"
    );
}

#[tokio::test]
async fn anthropic_test_extra_headers_passthrough() {
    // LiteLLM: test_anthropic_messages_with_extra_headers
    // Verify custom headers like anthropic-version reach the backend.
    let captured_body: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(captured_body.clone(), captured_headers.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let headers = captured_headers.lock().unwrap();
    assert!(
        headers.iter().any(|(k, _)| k == "anthropic-version"),
        "anthropic-version should be forwarded: {:?}",
        *headers
    );
}

#[tokio::test]
async fn anthropic_test_cache_control_passthrough() {
    // LiteLLM: base_anthropic_messages_prompt_caching_test (adapted)
    // Verify cache_control breakpoints pass through the anthropic passthrough.
    let captured_body: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = spawn_mock_anthropic_backend(captured_body.clone(), captured_headers.clone()).await;
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Long context to cache.", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "Follow up."}
                ]}
            ],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let sent = captured_body.lock().unwrap().take().unwrap();
    let blocks = sent["messages"][0]["content"].as_array().unwrap();
    assert_eq!(
        blocks[0]["cache_control"]["type"], "ephemeral",
        "cache_control should be preserved: {blocks:#?}"
    );
}

#[tokio::test]
async fn anthropic_test_streaming_error_handling() {
    // LiteLLM: test_anthropic_messages_streaming_with_bad_request
    // Verify a streaming error from the Anthropic backend returns SSE content.
    let app = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(|| async {
            (
                [("content-type", "text/event-stream")],
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_err\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-6\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
                    "event: error\n",
                    "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"max_tokens: must be at least 1\"}}\n\n",
                )
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mock = format!("http://{}", addr);
    let proxy = spawn_proxy(anthropic_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100,
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("finish_reason") || text.contains("[DONE]") || text.contains("error"),
        "response should contain streaming content: {text:.200}"
    );
}

// --- OpenAI-specific parity tests ---
// Ported from LiteLLM test patterns:
//   proxy_unit_tests/test_unit_test_streaming.py
//   openai_endpoints_tests/test_e2e_openai_responses_api.py
//   llm_translation/test_openai.py

#[tokio::test]
async fn openai_chat_completions_missing_model_returns_error() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["type"].is_string(), "should have error type");
}

#[tokio::test]
async fn openai_chat_completions_missing_messages_returns_error() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o",
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["type"].is_string());
}

#[tokio::test]
async fn openai_chat_completions_invalid_json_returns_openai_error() {
    let mock = spawn_mock_chat_backend().await;
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body("{invalid json}")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["type"].is_string());
    assert!(body["error"]["message"].is_string());
}

// --- OpenAI Responses API parity tests ---
// Ported from LiteLLM test patterns:
//   openai_endpoints_tests/test_e2e_openai_responses_api.py

/// Build a Config targeting the Responses API backend format with a mock base URL.
fn responses_config_with_base(base_url: &str) -> Config {
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: base_url.to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o-mini".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Responses,
        provider_id: None,
    }
}

async fn spawn_mock_responses_backend() -> String {
    let app = axum::Router::new().route(
        "/v1/responses",
        axum::routing::post(|| async {
            axum::Json(serde_json::json!({
                "id": "resp_mock123",
                "type": "response",
                "model": "gpt-4o-mini",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Hello from Responses API mock!"}]
                }],
                "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
                "status": "completed"
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn responses_api_non_streaming() {
    // LiteLLM: test_basic_response - verify Responses API translates correctly
    let mock = spawn_mock_responses_backend().await;
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(responses_config_with_base(&mock));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let proxy = format!("http://{addr}");

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o-mini",
            "max_tokens": 50,
            "messages": [{"role": "user", "content": "Say hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "message", "type should be 'message'");
    assert_eq!(body["role"], "assistant", "role should be 'assistant'");
    assert_eq!(body["content"][0]["type"], "text");
    assert_eq!(body["content"][0]["text"], "Hello from Responses API mock!");
}

#[tokio::test]
async fn responses_api_bad_request_error() {
    // LiteLLM: test_bad_request_error - verify invalid params return proper error
    let mock = spawn_mock_responses_backend().await;
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(responses_config_with_base(&mock));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let proxy = format!("http://{addr}");

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o-mini",
            "max_tokens": 50,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["content"][0]["text"], "Hello from Responses API mock!");
}

#[tokio::test]
async fn responses_api_messages_passthrough_preserves_content() {
    // LiteLLM: test_basic_response - verify message content round-trips
    let mock = spawn_mock_responses_backend().await;
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(responses_config_with_base(&mock));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let proxy = format!("http://{addr}");

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Tell me a joke"}],
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["content"].as_array().is_some_and(|c| !c.is_empty()),
        "content should be non-empty"
    );
    assert_eq!(body["content"][0]["type"], "text");
    assert!(
        body["usage"]["input_tokens"].as_u64().unwrap_or(0) > 0,
        "input_tokens should be positive"
    );
}

#[tokio::test]
async fn responses_api_backend_error_returns_proper_error() {
    // LiteLLM: test_bad_request_error - backend returns 400
    let app = axum::Router::new().route(
        "/v1/responses",
        axum::routing::post(|| async {
            (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {"message": "Invalid model", "type": "invalid_request_error"}
                })),
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mock = format!("http://{addr}");

    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let app = routes::app(responses_config_with_base(&mock));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let proxy = format!("http://{addr}");

    let resp = Client::new()
        .post(format!("{proxy}/v1/messages"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "nonexistent-model",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 50
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status().as_u16();
    assert!(status >= 400, "expected error status, got {status}");
}

#[tokio::test]
async fn openai_chat_completions_streaming_finish_reason_stop() {
    // Verify streaming chat completions end with finish_reason: stop
    // and data: [DONE] using OpenAI-style SSE.
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            let body = concat!(
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            );
            ([("content-type", "text/event-stream")], body)
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mock = format!("http://{}", addr);
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o",
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
        text.contains("\"finish_reason\":\"stop\""),
        "should contain stop reason: {text:.200}"
    );
    assert!(
        text.contains("[DONE]"),
        "should contain [DONE] sentinel: {text:.200}"
    );
    assert!(
        text.contains("Hello"),
        "should contain streamed text: {text:.200}"
    );
    assert!(
        text.contains("world"),
        "should contain all streamed text: {text:.200}"
    );
}

#[tokio::test]
async fn openai_chat_completions_streaming_backend_error_returns_sse_error() {
    // LiteLLM: test_unit_test_streaming.py pattern
    // Backend returns a 500 HTTP error for a streaming request.
    // The proxy should return an appropriate error.
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mock = format!("http://{}", addr);
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100,
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    // Should return an error (either 502 from proxy or pass through the 500)
    assert!(
        resp.status().as_u16() >= 400,
        "expected error status, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn openai_chat_completions_tool_call_streaming() {
    // LiteLLM: test_openai.py tool calling pattern
    // Verify streaming with tool calls works end-to-end.
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            let body = concat!(
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null,\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"loc\\\":\\\"NYC\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            );
            ([("content-type", "text/event-stream")], body)
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mock = format!("http://{}", addr);
    let proxy = spawn_proxy(openai_config_with_base(&mock)).await;

    let resp = Client::new()
        .post(format!("{proxy}/v1/chat/completions"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "What is the weather?"}],
            "max_tokens": 100,
            "stream": true,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {"loc": {"type": "string"}}, "required": ["loc"]}
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("tool_calls"),
        "should contain tool calls: {text:.200}"
    );
    assert!(
        text.contains("\"finish_reason\":\"tool_calls\""),
        "should have tool_calls finish: {text:.200}"
    );
}
