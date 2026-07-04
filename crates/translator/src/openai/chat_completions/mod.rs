// OpenAI Chat Completions request/response types

use serde::{Deserialize, Serialize};

// --- Request types ---

/// OpenAI Chat Completions API request body.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Stop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Maps from Anthropic metadata.user_id. Compat spec: "Ignored".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#simple-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Compat spec response: "Always empty". Present to avoid deserialization failure.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Captures OpenAI fields we don't need to translate (seed, logprobs,
    /// logit_bias, n, reasoning_effort, etc.) and forwards them as-is.
    /// Only fields requiring translation logic get explicit struct fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Options for streaming responses.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StreamOptions {
    #[serde(default)]
    pub include_usage: bool,
}

/// Stop sequence(s): single string or array.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Stop {
    Single(String),
    Multiple(Vec<String>),
}

/// A message in the conversation.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Compat spec response: "Always empty". Present to avoid deserialization failure.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    /// DeepSeek/Qwen thinking model output. Maps to/from Anthropic thinking blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// LiteLLM-compatible Anthropic thinking blocks. These preserve signatures
    /// and redacted thinking for tool-result continuations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<ThinkingBlock>>,
}

impl ChatMessage {
    /// Coalesce text from `content` and `reasoning_content`.
    ///
    /// Returns the first non-empty text found, in priority order:
    ///
    /// 1. Text from `content`: if it is `Text(s)`, returns `s`; if it is
    ///    `Parts`, joins all [`ChatContentPart::Text`] parts with `"\n"`
    ///    (non-text parts such as images are skipped). Whitespace-only strings
    ///    are returned as-is — this method does not trim.
    /// 2. `reasoning_content` as a fallback, if non-empty. Reasoning is metadata
    ///    output, not user-visible content, so it is never concatenated with (1).
    /// 3. `None` if both are absent or empty.
    pub fn effective_text(&self) -> Option<String> {
        let content_text = match &self.content {
            Some(ChatContent::Text(s)) if !s.is_empty() => Some(s.clone()),
            Some(ChatContent::Parts(parts)) => {
                let texts: Vec<&str> = parts
                    .iter()
                    .filter_map(|p| {
                        if let ChatContentPart::Text { text } = p {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                let joined = texts.join("\n");
                if !joined.is_empty() {
                    Some(joined)
                } else {
                    None
                }
            }
            _ => None,
        };

        content_text.or_else(|| {
            self.reasoning_content
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
        })
    }
}

/// LiteLLM-compatible copy of Anthropic thinking blocks on OpenAI messages.
///
/// `reasoning_content` is only text. These blocks preserve the exact signed or
/// redacted Anthropic state needed when a later tool result continues the turn.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ThinkingBlock {
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
    #[serde(other)]
    Unknown,
}

/// Message role: system, developer, user, assistant, or tool.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    /// Deprecated by OpenAI but still accepted. Compat spec lists function role messages.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#messages-array-fields
    Function,
}

/// Content can be a plain string or an array of typed content parts (multimodal).
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

/// Typed content part for multimodal messages.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: InputAudio },
    #[serde(rename = "file")]
    File { file: FileInput },
}

/// Image URL reference for vision requests.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Audio input for audio-capable models.
///
/// See <https://platform.openai.com/guides/audio>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct InputAudio {
    pub data: String,   // base64-encoded audio
    pub format: String, // "wav", "mp3", etc.
}

/// File input for file-capable models.
///
/// See <https://platform.openai.com/docs/guides/text>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FileInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>, // base64-encoded or data URI
}

// --- Tool call from assistant ---

/// Tool call from the assistant.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String, // always "function"
    pub function: FunctionCall,
}

/// Function name and JSON arguments string.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

// --- Tool definition ---

/// Tool definition wrapping a function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub tool_type: String, // always "function"
    pub function: FunctionDef,
}

/// Function definition with name, description, and parameters schema.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Compat spec: "Ignored". OpenAI accepts it; preserved for round-trip fidelity.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#tools--functions-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Tool choice: "auto", "none", "required", or named function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatToolChoice {
    Simple(String), // "auto", "none", "required"
    Named(NamedToolChoice),
}

/// Named tool choice specifying a specific function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedToolChoice {
    #[serde(rename = "type")]
    pub choice_type: String, // "function"
    pub function: NamedFunction,
}

/// Function name for named tool choice.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedFunction {
    pub name: String,
}

/// Response format: text, json_object, or json_schema.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

// --- Response types ---

/// OpenAI Chat Completions API response body.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
///
/// `id`, `object`, and `model` default to an empty string when absent so that
/// lax local OpenAI-compatible servers (llama.cpp, Ollama, etc.) that omit
/// these fields do not cause a deserialization failure.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub object: String, // "chat.completion"
    #[serde(default)]
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// A completion choice with message and finish reason.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

/// Why the model stopped: stop, length, tool_calls, content_filter, or function_call.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    FunctionCall,
    /// Mid-stream failure signalled by OpenAI-compatible gateways (notably
    /// OpenRouter) when a generation fails after a 200 SSE response has begun.
    /// Serializes as "error". Must precede the `#[serde(other)]` catch-all so
    /// the literal "error" binds here rather than falling through to `Unknown`.
    Error,
    /// Catch-all for provider-specific finish reasons (e.g. DeepSeek's
    /// "insufficient_system_resource"). Serializes as "unknown".
    #[serde(other)]
    Unknown,
}

/// Token usage: prompt, completion, and total.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
///
/// Token counters default to 0 when absent so that lax local servers that
/// omit usage do not cause a deserialization failure.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ChatUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    /// Compat spec response: "Always empty". OpenAI returns reasoning_tokens, etc.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<serde_json::Value>,
    /// Compat spec response: "Always empty". OpenAI returns cached_tokens, etc.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests;
