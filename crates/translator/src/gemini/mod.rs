/// Gemini generateContent API request types.
pub mod request;
/// Gemini generateContent API response types.
pub mod response;

// Re-export commonly used types
pub use request::{
    Content, FileData, FunctionCallData, FunctionCallingConfig, FunctionDeclaration,
    FunctionResponseData, GenerateContentRequest, GenerationConfig, InlineData, Part,
    SafetySetting, Tool, ToolConfig,
};
pub use response::{Candidate, FinishReason, GenerateContentResponse, SafetyRating, UsageMetadata};
