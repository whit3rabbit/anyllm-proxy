/// OpenAI Chat Completions API request and response types.
pub mod chat_completions;
/// OpenAI error response types.
pub mod errors;
/// OpenAI Responses API types.
pub mod responses;
/// OpenAI Chat Completions SSE streaming chunk types.
pub mod streaming;
/// OpenAI-compatible tool-call wire normalization helpers.
pub mod tool_normalization;

pub use chat_completions::{
    ChatCompletionRequest, ChatCompletionResponse, ChatContent, ChatContentPart, ChatMessage,
    ChatRole, ChatTool, ChatToolChoice, ChatUsage, Choice, FinishReason, FunctionCall, FunctionDef,
    Stop, StreamOptions, ThinkingBlock, ToolCall,
};
pub use errors::{ErrorDetail, ErrorResponse};
pub use streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
};
