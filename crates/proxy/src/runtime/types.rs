//! Runtime result, metadata, and service-trait types.

use super::error::ChatCompletionError;
use crate::backend::RateLimitHeaders;
use crate::config::{BackendKind, OpenAIApiFormat};
use anyllm_translate::{openai, TranslationWarnings};
use futures::{future::BoxFuture, Stream};
use std::fmt;
use std::pin::Pin;

/// Stream returned by [`ChatCompletionRuntime::complete_stream`].
pub type ChatCompletionChunkStream =
    Pin<Box<dyn Stream<Item = Result<openai::ChatCompletionChunk, ChatCompletionError>> + Send>>;

/// Object-safe service API for one Chat Completions call at a time.
pub trait ChatCompletionService: Send + Sync {
    fn complete<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionResult, ChatCompletionError>>;

    fn complete_stream<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionStreamResult, ChatCompletionError>>;
}

/// Non-streaming runtime response.
#[derive(Debug)]
pub struct ChatCompletionResult {
    pub response: openai::ChatCompletionResponse,
    pub usage: Option<openai::ChatUsage>,
    pub rate_limits: RateLimitHeaders,
    pub metadata: ChatCompletionMetadata,
    pub warnings: TranslationWarnings,
}

/// Streaming runtime response.
pub struct ChatCompletionStreamResult {
    pub chunks: ChatCompletionChunkStream,
    pub rate_limits: RateLimitHeaders,
    pub metadata: ChatCompletionMetadata,
    pub warnings: TranslationWarnings,
}

impl fmt::Debug for ChatCompletionStreamResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatCompletionStreamResult")
            .field("chunks", &"<stream>")
            .field("rate_limits", &self.rate_limits)
            .field("metadata", &self.metadata)
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Backend selection metadata for observability and callers that need accounting.
#[derive(Debug, Clone)]
pub struct ChatCompletionMetadata {
    pub requested_model: String,
    pub selected_backend: String,
    pub mapped_model: String,
    pub backend_kind: BackendKind,
    pub provider_id: Option<String>,
    pub api_format: OpenAIApiFormat,
    pub used_responses_api: bool,
}
