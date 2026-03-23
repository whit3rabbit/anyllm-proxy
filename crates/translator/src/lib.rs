//! # anthropic_openai_translate
//!
//! Pure, IO-free translation between Anthropic Messages API and OpenAI Chat Completions API.
//!
//! # Quick start
//!
//! ```rust
//! use anthropic_openai_translate::{TranslationConfig, translate_request, translate_response};
//! use anthropic_openai_translate::anthropic::MessageCreateRequest;
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

pub mod anthropic;
pub mod config;
pub mod error;
pub mod mapping;
#[cfg(feature = "middleware")]
pub mod middleware;
pub mod openai;
pub mod translate;
pub mod util;

// Convenience re-exports
pub use config::{LossyBehavior, TranslationConfig, TranslationConfigBuilder};
pub use error::TranslateError;
pub use translate::{
    new_responses_stream_translator, new_stream_translator, translate_request,
    translate_request_responses, translate_response, translate_response_responses,
};
