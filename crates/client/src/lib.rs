//! # anyllm_client
//!
//! Async HTTP client for Anthropic-to-OpenAI API translation.
//!
//! Accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions
//! format, sends them to an OpenAI-compatible backend, and translates the response back.
//! Supports non-streaming and streaming (SSE) modes, retry with exponential backoff,
//! SSRF-safe DNS resolution, and mTLS.
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
//! - [`tools`] -- Builder helpers for [`Tool`] definitions and [`ToolChoice`]
//! - [`http`] -- HTTP client builder with TLS and SSRF protection
//! - [`retry`] -- Generic retry logic with exponential backoff
//! - [`rate_limit`] -- Rate limit header extraction and format conversion
//! - [`sse`] -- Framework-agnostic SSE frame parser
//! - [`error`] -- Error types

pub mod client;
pub mod error;
pub mod http;
pub mod rate_limit;
pub mod retry;
pub mod sse;
pub(crate) mod streaming;
pub mod tools;

// Convenience re-exports
pub use client::{Auth, Client, ClientBuilder, ClientConfig, ClientConfigBuilder};
pub use error::ClientError;
pub use http::{build_http_client, HttpClientConfig};
pub use rate_limit::RateLimitHeaders;
pub use retry::{backoff_delay, is_retryable, parse_retry_after, send_with_retry, RetryableError};
pub use sse::{find_double_newline, SseError};
pub use tools::{ToolBuilder, ToolChoiceBuilder};

// Re-export key types from the translator crate so downstream users
// do not need a direct dependency on `anyllm_translate`.
pub use anyllm_translate::anthropic::streaming::StreamEvent;
pub use anyllm_translate::anthropic::{Tool, ToolChoice};
