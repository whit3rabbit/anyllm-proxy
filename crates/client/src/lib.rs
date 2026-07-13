//! # anyllm_client
//!
//! Async HTTP client for Anthropic-to-OpenAI API translation.
//!
//! Accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions
//! format, sends them to an OpenAI-compatible backend, and translates the response back.
//! Supports non-streaming and streaming (SSE) modes, retry with exponential backoff,
//! SSRF-safe DNS resolution, and mTLS.
//!
//! A native Anthropic Messages passthrough client ([`AnthropicMessagesClient`]) is also
//! available for forwarding requests directly to the Anthropic API without translation.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use anyllm_client::{Client, ClientConfig, Auth};
//! use anyllm_translate::TranslationConfig;
//! use anyllm_translate::anthropic::MessageCreateRequest;
//!
//! # async fn example() -> Result<(), anyllm_client::ClientError> {
//! let config = ClientConfig::builder()
//!     .backend_url("https://api.openai.com/v1/chat/completions")
//!     .auth(Auth::Bearer("sk-...".into()))
//!     .translation(
//!         TranslationConfig::builder()
//!             .model_map("haiku", "gpt-4o-mini")
//!             .model_map("sonnet", "gpt-4o")
//!             .build()
//!     )
//!     .build();
//!
//! let client = Client::new(config);
//!
//! let req: MessageCreateRequest = serde_json::from_str(r#"{
//!     "model": "claude-sonnet-4-6",
//!     "max_tokens": 100,
//!     "messages": [{"role": "user", "content": "Hello"}]
//! }"#).unwrap();
//!
//! let response = client.messages(&req).await?;
//! println!("{:?}", response);
//! # Ok(())
//! # }
//! ```
//!
//! # Modules
//!
//! - [`client`] -- High-level `Client` and [`ClientBuilder`] for Anthropic-in, Anthropic-out API calls
//! - [`anthropic_client`] -- Native Anthropic Messages API passthrough client
//! - [`tools`] -- Builder helpers for [`Tool`] definitions and [`ToolChoice`]
//! - [`http`] -- HTTP client builder with TLS and SSRF protection
//! - [`retry`] -- Generic retry logic with exponential backoff
//! - [`rate_limit`] -- Rate limit header extraction and format conversion
//! - [`sse`] -- Framework-agnostic SSE frame parser
//! - [`error`] -- Error types

// Ensure at least one TLS backend is compiled in. Without this, reqwest will
// fail at runtime when trying to make HTTPS connections, with an unhelpful
// error message. Enable either the `native-tls` feature (the default) or
// the `rustls` feature in your Cargo.toml dependency.
#[cfg(not(any(feature = "native-tls", feature = "rustls")))]
compile_error!(
    "anyllm_client requires a TLS backend. \
     Enable the `native-tls` feature (default) or the `rustls` feature in your Cargo.toml: \
     anyllm_client = { features = [\"native-tls\"] }"
);

/// Native Anthropic Messages API passthrough client.
pub mod anthropic_client;
/// High-level HTTP Client for routing translated requests to OpenAI-compatible backends.
pub mod client;
/// Error definitions for the client.
pub mod error;
/// HTTP client builder and SSL/SSRF protection mechanisms.
pub mod http;
/// Rate limit headers parsing and format conversion.
pub mod rate_limit;
/// Retry with exponential backoff mechanisms.
pub mod retry;
/// SSE frame parser.
pub mod sse;
pub(crate) mod streaming;
/// Tools builder helpers.
pub mod tools;

// Convenience re-exports
pub use anthropic_client::AnthropicMessagesClient;
pub use client::{Auth, Client, ClientBuilder, ClientConfig, ClientConfigBuilder};
pub use error::ClientError;
pub use http::{build_http_client, HttpClientConfig};
pub use rate_limit::RateLimitHeaders;
pub use retry::{
    backoff_delay, is_quota_exhausted, is_retryable, parse_retry_after, send_with_retry,
    send_with_retry_policy, RetryPolicy, RetryableError,
};
pub use sse::{find_double_newline, SseError, SseFrameBuffer};
pub use tools::{ToolBuilder, ToolChoiceBuilder};

// Re-export key types from the translator crate so downstream users
// do not need a direct dependency on `anyllm_translate`.
//
// Anthropic types (root-level — existing API):
pub use anyllm_translate::anthropic::streaming::StreamEvent;
pub use anyllm_translate::anthropic::{Tool, ToolChoice};

// Module re-exports for full type access without a separate anyllm_translate dependency.
// Use `anyllm_client::openai::ChatMessage` etc. to avoid collisions with Anthropic
// root-level types (Tool, ToolChoice). Root-level aliases are provided only for
// types that appear in public method signatures.
pub use anyllm_translate::anthropic;
pub use anyllm_translate::openai;

// Root-level OpenAI type aliases — the types used by chat_completion()'s public signature.
pub use anyllm_translate::openai::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
};
