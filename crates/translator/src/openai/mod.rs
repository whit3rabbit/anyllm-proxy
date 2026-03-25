/// OpenAI Chat Completions API request and response types.
pub mod chat_completions;
/// OpenAI error response types.
pub mod errors;
/// OpenAI Responses API types.
pub mod responses;
/// OpenAI Chat Completions SSE streaming chunk types.
pub mod streaming;

pub use chat_completions::{
    ChatCompletionRequest, ChatCompletionResponse, ChatContent, ChatContentPart, ChatMessage,
    ChatRole, ChatTool, ChatToolChoice, ChatUsage, Choice, FinishReason, FunctionCall, FunctionDef,
    Stop, StreamOptions, ToolCall,
};
pub use errors::{ErrorDetail, ErrorResponse};
pub use streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
};
