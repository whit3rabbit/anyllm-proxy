---
title: "anyllm_client"
description: "Reference for the async HTTP client crate, including builders, transport config, retry helpers, and tool builders."
---

Source files: `crates/client/src/lib.rs`, `crates/client/src/client.rs`, `crates/client/src/http.rs`, `crates/client/src/retry.rs`, `crates/client/src/tools.rs`

## Import Path

```rust
use anyllm_client::{
    Auth, Client, ClientBuilder, ClientConfig, ClientConfigBuilder,
    HttpClientConfig, build_http_client,
    ToolBuilder, ToolChoiceBuilder,
    backoff_delay, is_retryable, parse_retry_after, send_with_retry,
    MAX_RETRIES,
};
```

## Core Configuration

```rust
pub enum Auth {
    Bearer(String),
    Header { name: String, value: String },
}

pub struct ClientConfig {
    pub chat_completions_url: String,
    pub auth: Auth,
    pub http: HttpClientConfig,
    pub translation: TranslationConfig,
}

pub struct HttpClientConfig {
    pub p12_identity: Option<(Vec<u8>, zeroize::Zeroizing<String>)>,
    pub ca_cert_pem: Option<Vec<u8>>,
    pub connect_timeout: Option<Duration>,
    pub request_timeout: Option<Duration>,
    pub read_timeout: Option<Duration>,
    pub tcp_keepalive: Option<Duration>,
    pub ssrf_protection: bool,
}
```

### `ClientConfigBuilder`

| Method | Signature | Default behavior |
|---|---|---|
| `backend_url` | `pub fn backend_url(self, url: impl Into<String>) -> Self` | Required URL input. |
| `auth` | `pub fn auth(self, auth: Auth) -> Self` | Defaults to empty bearer token if omitted. |
| `http` | `pub fn http(self, http: HttpClientConfig) -> Self` | Uses secure defaults if omitted. |
| `translation` | `pub fn translation(self, translation: TranslationConfig) -> Self` | Uses `TranslationConfig::default()` if omitted. |
| `build` | `pub fn build(self) -> ClientConfig` | Finalizes the config. |

### `ClientBuilder`

| Method | Signature | Notes |
|---|---|---|
| `new` | `pub fn new() -> Self` | Empty builder. |
| `base_url` | `pub fn base_url(self, url: &str) -> Self` | Required for `build()`. |
| `api_key` | `pub fn api_key(self, key: &str) -> Self` | Rejects empty strings. |
| `connect_timeout` | `pub fn connect_timeout(self, duration: Duration) -> Self` | Overrides the 10s default. |
| `timeout` | `pub fn timeout(self, duration: Duration) -> Self` | Sets total request timeout. |
| `read_timeout` | `pub fn read_timeout(self, duration: Duration) -> Self` | Overrides the 900s default. |
| `max_retries` | `pub fn max_retries(self, n: u32) -> Self` | Overrides `MAX_RETRIES`. |
| `build` | `pub fn build(self) -> Result<Client, ClientError>` | Builds a ready client. |

## `Client`

```rust
pub fn new(config: ClientConfig) -> Self
pub fn builder() -> ClientBuilder
pub fn with_http_client(http: reqwest::Client, config: ClientConfig) -> Self
pub async fn messages(
    &self,
    req: &MessageCreateRequest,
) -> Result<MessageResponse, ClientError>
pub async fn messages_stream(
    &self,
    req: &MessageCreateRequest,
) -> Result<
    (
        impl Stream<Item = Result<StreamEvent, ClientError>>,
        RateLimitHeaders,
    ),
    ClientError,
>
pub async fn chat_completion(
    &self,
    req: &ChatCompletionRequest,
) -> Result<(ChatCompletionResponse, u16, RateLimitHeaders), ClientError>
```

### Method details

| Method | Parameters | Return type | Source |
|---|---|---|---|
| `messages` | `req: &MessageCreateRequest` | `Result<MessageResponse, ClientError>` | `crates/client/src/client.rs` |
| `messages_stream` | `req: &MessageCreateRequest` | Anthropic SSE stream plus rate-limit headers | `crates/client/src/client.rs` |
| `chat_completion` | `req: &ChatCompletionRequest` | translated response body, status, and rate-limit headers | `crates/client/src/client.rs` |

## HTTP Helpers

```rust
pub fn build_http_client(config: &HttpClientConfig) -> reqwest::Client
pub fn is_private_ip(ip: IpAddr) -> bool
```

`build_http_client` applies hardened defaults from `crates/client/src/http.rs`: 10-second connect timeout, 900-second read timeout, 60-second TCP keepalive, optional mTLS, optional custom CA, and SSRF-safe DNS resolution when the `ssrf-protection` feature is enabled.

## Retry Helpers

```rust
pub const MAX_RETRIES: u32 = 3
pub const BASE_DELAY_MS: u64 = 500

pub trait RetryableError: Sized {
    fn from_request(e: reqwest::Error) -> Self;
    fn from_api_response(status: u16, body: &str) -> Self;
}

pub async fn send_with_retry<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &RequestAuth<'_>,
    body: &impl Serialize,
    label: &str,
    max_retries: u32,
) -> Result<reqwest::Response, E>
pub fn is_retryable(status: u16) -> bool
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration>
pub fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration
```

## Tool Builders

```rust
pub struct ToolBuilder
pub fn new(name: &str) -> Self
pub fn description(self, desc: &str) -> Self
pub fn input_schema(self, schema: Value) -> Self
pub fn build(self) -> Tool

pub struct ToolChoiceBuilder
pub fn auto() -> ToolChoice
pub fn any() -> ToolChoice
pub fn none() -> ToolChoice
pub fn specific(name: &str) -> ToolChoice
```

## Example

```rust
use anyllm_client::Client;
use anyllm_translate::anthropic::MessageCreateRequest;

let client = Client::builder()
    .base_url("https://api.openai.com/v1/chat/completions")
    .api_key("sk-...")
    .build()?;

let req: MessageCreateRequest = serde_json::from_str(r#"{
  "model": "claude-3-5-sonnet-latest",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "hello"}]
}"#)?;

let resp = client.messages(&req).await?;
println!("{:?}", resp.content);
# Ok::<(), Box<dyn std::error::Error>>(())
```
