//! In-process Chat Completions runtime.
//!
//! This module exposes model routing and backend dispatch without taking
//! ownership of HTTP routing, auth, admin UI, caching, or tool execution.
//!
//! - [`error`]   runtime error type
//! - [`types`]   result / metadata / service-trait types
//! - [`service`] the `ChatCompletionRuntime` implementation
//! - [`stream`]  SSE chunk stream adapters

mod error;
mod service;
pub mod stream;
mod types;

pub use error::ChatCompletionError;
pub use service::ChatCompletionRuntime;
pub use types::{
    ChatCompletionChunkStream, ChatCompletionMetadata, ChatCompletionResult, ChatCompletionService,
    ChatCompletionStreamResult,
};
