/// Anthropic Message Batches API request/response types.
pub mod batch;
/// Anthropic error response types (`ErrorResponse`, `ErrorType`, `ErrorDetail`).
pub mod errors;
/// Anthropic Messages API request and response types.
pub mod messages;
/// Anthropic SSE streaming event types (`StreamEvent`, `Delta`).
pub mod streaming;

// Re-export primary types
pub use errors::{ErrorDetail, ErrorResponse, ErrorType};
pub use messages::{
    CacheControl, Content, ContentBlock, DocumentSource, ImageSource, InputMessage,
    MessageCreateRequest, MessageResponse, Metadata, Role, StopReason, System, SystemBlock,
    ThinkingConfig, Tool, ToolChoice, ToolResultContent, Usage,
};
pub use streaming::{Delta, StreamEvent};
