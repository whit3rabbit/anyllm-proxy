//! # anyllm_translate
//!
//! Pure, IO-free translation between Anthropic Messages API and OpenAI Chat Completions API.
//!
//! # Quick start
//!
//! ```rust
//! use anyllm_translate::{TranslationConfig, translate_request, translate_response};
//! use anyllm_translate::anthropic::MessageCreateRequest;
//!
//! let config = TranslationConfig::builder()
//!     .model_map("haiku", "gpt-4o-mini")
//!     .model_map("sonnet", "gpt-4o")
//!     .model_map("opus", "gpt-4o")
//!     .build();
//!
//! let req: MessageCreateRequest = serde_json::from_str(r#"{
//!     "model": "claude-sonnet-4-6",
//!     "max_tokens": 100,
//!     "messages": [{"role": "user", "content": "Hello"}]
//! }"#).unwrap();
//!
//! let openai_req = translate_request(&req, &config).unwrap();
//! assert_eq!(openai_req.model, "gpt-4o");
//!
//! // ... send openai_req to OpenAI, get response ...
//! // let anthropic_resp = translate_response(&openai_resp, &req.model);
//! ```
//!
//! # Modules
//!
//! - [`anthropic`] -- Anthropic Messages API types
//! - [`openai`] -- OpenAI Chat Completions and Responses API types
//! - [`mapping`] -- Stateless conversion functions between APIs
//! - [`config`] -- Translation configuration (model mapping, lossy behavior)
//! - [`translate`] -- Convenience wrappers combining config with mapping functions

/// Anthropic Messages API types (request, response, streaming events, errors).
pub mod anthropic;
/// Translation configuration: model mapping and lossy-translation behavior.
pub mod config;
/// Error types for translation failures.
pub mod error;
/// Gemini native generateContent API types (request, response).
pub mod gemini;
/// Stateless conversion functions between Anthropic and OpenAI API formats.
pub mod mapping;
/// HTTP middleware for request/response translation (requires `middleware` feature).
#[cfg(feature = "middleware")]
pub mod middleware;
/// OpenAI Chat Completions and Responses API types.
pub mod openai;
/// Convenience wrappers combining config with mapping functions.
pub mod translate;
/// Shared utilities: ID generation, JSON helpers, secret redaction.
pub mod util;

// Convenience re-exports
pub use config::{LossyBehavior, TranslationConfig, TranslationConfigBuilder};
pub use error::TranslateError;
pub use mapping::reverse_streaming_map::ReverseStreamingTranslator;
pub use translate::{
    compute_request_warnings, new_responses_stream_translator, new_reverse_stream_translator,
    new_stream_translator, translate_anthropic_to_openai_response,
    translate_openai_to_anthropic_request, translate_request, translate_request_responses,
    translate_response, translate_response_responses, TranslationWarnings,
};
