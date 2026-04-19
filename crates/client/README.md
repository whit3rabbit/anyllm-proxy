# anyllm_client

Async HTTP client that accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions, sends them to any OpenAI-compatible backend, and translates the response back to Anthropic format. Part of the [anyllm-proxy](https://github.com/whit3rabbit/anyllm-proxy) workspace.

## What this crate is

A self-contained client library, not a CLI or server. Use it when you want Anthropic-shaped requests and responses in your own Rust code without running the proxy as a sidecar.

It owns:

- A `reqwest`-based HTTP client with TLS, mTLS, and (by default) SSRF-safe DNS resolution.
- Retry with exponential backoff and `Retry-After` parsing.
- A framework-agnostic SSE frame parser for streaming responses.
- Anthropic-shaped tool builders (`ToolBuilder`, `ToolChoiceBuilder`).
- Rate-limit header extraction and conversion between vendor formats.

It deliberately does **not** own:

- The format mapping itself: that lives in `anyllm_translate` and is re-exported where it makes sense.
- A queue, batch engine, or admin UI: those are separate crates.

## Where it fits

Five-crate workspace:

- `anyllm_translate` - pure format mapping, no I/O.
- `anyllm_providers` - provider and model catalog.
- `anyllm_client` (this crate) - async HTTP client wrapping translate + transport.
- `anyllm_batch_engine` - batch job queue and webhook delivery.
- `anyllm_proxy` - axum HTTP server, admin UI, config parsing.

Depend on this crate directly when you need Anthropic-in / Anthropic-out from inside your own application. Depend on `anyllm_proxy` when you want a standalone HTTP server with config files, an admin UI, virtual keys, and metrics.

## Add it

```toml
[dependencies]
anyllm_client = "0.9"
```

Default features include `ssrf-protection`. Disable it only for local development against `127.0.0.1` backends:

```toml
anyllm_client = { version = "0.9", default-features = false }
```

## Library examples

### 1. Minimal: builder shorthand

The `Client::builder()` shorthand is the smallest path from "I have an API key" to "I have a working client". It defaults `Auth` to `Bearer` and uses the default `TranslationConfig` (1:1 model passthrough).

```rust
use anyllm_client::Client;
use anyllm_translate::anthropic::MessageCreateRequest;

let client = Client::builder()
    .base_url("https://api.openai.com/v1/chat/completions")
    .api_key(&std::env::var("OPENAI_API_KEY")?)
    .build()?;

let req: MessageCreateRequest = serde_json::from_str(r#"{
    "model": "gpt-4o-mini",
    "max_tokens": 256,
    "messages": [{"role": "user", "content": "Summarize Rust's borrow checker"}]
}"#)?;

let resp = client.messages(&req).await?;
println!("{:#?}", resp.content);
```

### 2. Anthropic-shaped clients on top of OpenAI-compatible providers

If your application speaks Anthropic Messages but you want to point it at Groq, OpenRouter, Together, or any local OpenAI-compatible server, use `ClientConfig::builder()` with a `TranslationConfig` that maps your Anthropic model aliases onto whatever the backend actually serves.

```rust
use anyllm_client::{Auth, Client, ClientConfig};
use anyllm_translate::TranslationConfig;

let translation = TranslationConfig::builder()
    .model_map("claude-3-5-haiku-latest",  "llama-3.1-8b-instant")
    .model_map("claude-3-5-sonnet-latest", "llama-3.3-70b-versatile")
    .build();

let client = Client::new(
    ClientConfig::builder()
        .backend_url("https://api.groq.com/openai/v1/chat/completions")
        .auth(Auth::Bearer(std::env::var("GROQ_API_KEY")?.into()))
        .translation(translation)
        .build(),
);
```

The same shape works for `http://localhost:11434/v1/chat/completions` (Ollama) or `http://localhost:1234/v1/chat/completions` (LM Studio). For keyless local backends, pass `Auth::Bearer("".into())`.

### 3. Streaming SSE

`messages_stream` returns a stream of Anthropic-shaped `StreamEvent` values. Translation happens incrementally so you can render tokens as they arrive.

```rust
use futures::StreamExt;
use anyllm_translate::anthropic::streaming::{Delta, StreamEvent};

let (mut stream, _rate_limits) = client.messages_stream(&req).await?;
while let Some(event) = stream.next().await {
    match event? {
        StreamEvent::ContentBlockDelta { delta: Delta::TextDelta { text }, .. } => {
            print!("{text}");
        }
        StreamEvent::MessageStop {} => break,
        _ => {}
    }
}
```

### 4. Tool use with the fluent builders

`ToolBuilder` and `ToolChoiceBuilder` produce Anthropic-shaped tool definitions without raw JSON.

```rust
use anyllm_client::{ToolBuilder, ToolChoiceBuilder};
use serde_json::json;

let weather = ToolBuilder::new("get_weather")
    .description("Get the current weather for a location")
    .input_schema(json!({
        "type": "object",
        "properties": { "location": { "type": "string" } },
        "required": ["location"]
    }))
    .build();

let mut req: anyllm_translate::anthropic::MessageCreateRequest =
    serde_json::from_str(r#"{
        "model": "claude-3-5-sonnet-latest",
        "max_tokens": 512,
        "messages": [{"role": "user", "content": "What's the weather in Paris?"}]
    }"#)?;
req.tools = Some(vec![weather]);
req.tool_choice = Some(ToolChoiceBuilder::auto());

let resp = client.messages(&req).await?;
```

### 5. Custom auth header (Azure, custom gateways)

Backends that want `api-key:` instead of `Authorization: Bearer` use `Auth::Header`.

```rust
use anyllm_client::{Auth, Client, ClientConfig};

let client = Client::new(
    ClientConfig::builder()
        .backend_url("https://my-resource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21")
        .auth(Auth::Header { name: "api-key".into(), value: std::env::var("AZURE_OPENAI_KEY")?.into() })
        .build(),
);
```

### 6. Tuning timeouts and retries

```rust
use std::time::Duration;
use anyllm_client::Client;

let client = Client::builder()
    .base_url("https://api.openai.com/v1/chat/completions")
    .api_key(&std::env::var("OPENAI_API_KEY")?)
    .connect_timeout(Duration::from_secs(5))
    .read_timeout(Duration::from_secs(120))
    .max_retries(5)
    .build()?;
```

Retries fire on 429 and 5xx with exponential backoff and `Retry-After` honoring. The retry helpers (`backoff_delay`, `is_retryable`, `parse_retry_after`, `send_with_retry`) are re-exported if you want to reuse them in your own HTTP code.

### 7. Sharing one `reqwest::Client` across many `Client`s

Useful when you fan out requests to multiple backends and want a single connection pool.

```rust
use anyllm_client::{build_http_client, Auth, Client, ClientConfig, HttpClientConfig};

let http = build_http_client(&HttpClientConfig::new());

let openai = Client::with_http_client(http.clone(), ClientConfig::builder()
    .backend_url("https://api.openai.com/v1/chat/completions")
    .auth(Auth::Bearer(std::env::var("OPENAI_API_KEY")?.into()))
    .build());

let groq = Client::with_http_client(http, ClientConfig::builder()
    .backend_url("https://api.groq.com/openai/v1/chat/completions")
    .auth(Auth::Bearer(std::env::var("GROQ_API_KEY")?.into()))
    .build());
```

## Modules

| Module | Purpose |
|---|---|
| `client` | High-level `Client`, `ClientBuilder`, `ClientConfig`, `Auth`. |
| `http` | HTTP client builder with TLS and SSRF protection. |
| `retry` | Generic retry with exponential backoff and `Retry-After` parsing. |
| `rate_limit` | Vendor rate-limit header extraction and format conversion. |
| `sse` | Framework-agnostic SSE frame parser. |
| `tools` | Builder helpers for `Tool` and `ToolChoice`. |
| `error` | `ClientError` and related types. |

## Tests

```bash
cargo test -p anyllm_client
```
