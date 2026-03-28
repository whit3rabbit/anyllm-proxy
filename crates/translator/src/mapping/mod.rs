/// Anthropic batch JSONL <-> OpenAI batch JSONL translation functions.
pub mod batch_map;
/// HTTP status and error shape translation between APIs.
pub mod errors_map;
/// Anthropic Messages API <-> Gemini generateContent API message mapping.
pub mod gemini_message_map;
/// Gemini streaming: full-response diffing -> Anthropic SSE delta events.
pub mod gemini_streaming_map;
/// Message and content block translation (system prompt, text, images, documents).
pub mod message_map;
/// Anthropic to/from OpenAI Responses API request and response mapping.
pub mod responses_message_map;
/// Responses API SSE event stream translation state machine.
pub mod responses_streaming_map;
/// Reverse message mapping: OpenAI Chat Completions -> Anthropic Messages.
pub mod reverse_message_map;
/// Reverse streaming: Anthropic SSE events -> OpenAI ChatCompletionChunk SSE.
pub mod reverse_streaming_map;
/// Chat Completions SSE event stream translation state machine.
pub mod streaming_map;
/// Tool definitions and tool_use/tool_call translation.
pub mod tools_map;
/// Token usage field mapping between Anthropic and OpenAI formats.
pub mod usage_map;
/// Degradation warning collection for client-visible feature-drop signals.
pub mod warnings;

/// Format an OpenAI refusal string as Anthropic text content.
/// Anthropic has no refusal type, so we surface it as a bracketed text marker.
pub(crate) fn format_refusal(refusal: &str) -> String {
    format!("[Refusal: {}]", refusal)
}
