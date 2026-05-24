---
title: "anyllm_translate"
description: "Reference for the pure translation crate that maps Anthropic Messages to OpenAI Chat Completions, OpenAI Responses, and Gemini native formats."
---

Source files: `crates/translator/src/lib.rs`, `crates/translator/src/config.rs`, `crates/translator/src/translate.rs`, `crates/translator/src/anthropic/messages.rs`, `crates/translator/src/openai/chat_completions.rs`

## Import Path

```rust
use anyllm_translate::{
    LossyBehavior, TranslationConfig, TranslationConfigBuilder,
    compute_request_warnings, translate_request, translate_response,
    translate_request_responses, translate_response_responses,
    translate_openai_to_anthropic_request, translate_anthropic_to_openai_response,
    translate_request_gemini, translate_response_gemini,
    new_stream_translator, new_responses_stream_translator,
    new_reverse_stream_translator, new_gemini_stream_translator,
};
```

## Core Types

```rust
pub enum LossyBehavior {
    Silent,
    Warn,
    Error,
}

pub struct TranslationConfig {
    pub model_map: Vec<(String, String)>,
    pub lossy_behavior: LossyBehavior,
    pub passthrough_unknown_models: bool,
}

pub struct TranslationConfigBuilder { /* builder state */ }
```

### `TranslationConfig` methods

```rust
pub fn builder() -> TranslationConfigBuilder
pub fn map_model(&self, model: &str) -> Result<String, TranslateError>
```

### `TranslationConfigBuilder` methods

| Method | Signature | Description |
|---|---|---|
| `model_map` | `pub fn model_map(self, pattern: impl Into<String>, target: impl Into<String>) -> Self` | Adds an ordered, case-insensitive substring rule. |
| `lossy_behavior` | `pub fn lossy_behavior(self, behavior: LossyBehavior) -> Self` | Controls whether unsupported features are dropped, warned, or rejected. |
| `passthrough_unknown_models` | `pub fn passthrough_unknown_models(self, passthrough: bool) -> Self` | Allows or rejects unmapped model ids. |
| `build` | `pub fn build(self) -> TranslationConfig` | Finalizes the builder. |

## Top-Level Functions

```rust
pub fn compute_request_warnings(req: &MessageCreateRequest) -> TranslationWarnings
pub fn translate_request(
    req: &MessageCreateRequest,
    config: &TranslationConfig,
) -> Result<ChatCompletionRequest, TranslateError>
pub fn translate_response(
    resp: &ChatCompletionResponse,
    original_model: &str,
) -> MessageResponse
pub fn new_stream_translator(model: String) -> StreamingTranslator
pub fn translate_request_responses(
    req: &MessageCreateRequest,
    config: &TranslationConfig,
) -> Result<ResponsesRequest, TranslateError>
pub fn translate_response_responses(
    resp: &ResponsesResponse,
    original_model: &str,
) -> MessageResponse
pub fn translate_openai_to_anthropic_request(
    req: &ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) -> Result<MessageCreateRequest, TranslateError>
pub fn translate_anthropic_to_openai_response(
    resp: &MessageResponse,
    model: &str,
) -> ChatCompletionResponse
pub fn new_reverse_stream_translator(
    id: String,
    model: String,
) -> ReverseStreamingTranslator
pub fn new_responses_stream_translator(
    model: String,
) -> ResponsesStreamingTranslator
pub fn translate_request_gemini(
    req: &MessageCreateRequest,
    config: &TranslationConfig,
) -> Result<(GenerateContentRequest, String), TranslateError>
pub fn translate_response_gemini(
    resp: &GenerateContentResponse,
    model: &str,
) -> MessageResponse
pub fn new_gemini_stream_translator(model: String) -> GeminiStreamingTranslator
```

### Parameters

| Function | Key parameters | Return type |
|---|---|---|
| `translate_request` | `req: &MessageCreateRequest`, `config: &TranslationConfig` | `Result<ChatCompletionRequest, TranslateError>` |
| `translate_response` | `resp: &ChatCompletionResponse`, `original_model: &str` | `MessageResponse` |
| `translate_request_responses` | `req: &MessageCreateRequest`, `config: &TranslationConfig` | `Result<ResponsesRequest, TranslateError>` |
| `translate_openai_to_anthropic_request` | `req: &ChatCompletionRequest`, `warnings: &mut TranslationWarnings` | `Result<MessageCreateRequest, TranslateError>` |
| `translate_request_gemini` | `req: &MessageCreateRequest`, `config: &TranslationConfig` | `Result<(GenerateContentRequest, String), TranslateError>` |

## Key Request And Response Types

```rust
pub struct MessageCreateRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<InputMessage>,
    pub system: Option<System>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub stop_sequences: Option<Vec<String>>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
    pub metadata: Option<Metadata>,
    pub thinking: Option<ThinkingConfig>,
    pub stream: Option<bool>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

pub enum ToolChoice {
    Auto { disable_parallel_tool_use: Option<bool> },
    Any { disable_parallel_tool_use: Option<bool> },
    None,
    Tool { name: String },
}
```

```rust
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub max_completion_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Stop>,
    pub tools: Option<Vec<ChatTool>>,
    pub tool_choice: Option<ChatToolChoice>,
    pub stream: Option<bool>,
    pub stream_options: Option<StreamOptions>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub response_format: Option<ResponseFormat>,
    pub user: Option<String>,
    pub parallel_tool_calls: Option<bool>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

## Example

```rust
use anyllm_translate::{TranslationConfig, translate_request};
use anyllm_translate::anthropic::MessageCreateRequest;

let cfg = TranslationConfig::builder()
    .model_map("sonnet", "gpt-4o")
    .build();

let req: MessageCreateRequest = serde_json::from_str(r#"{
  "model": "claude-3-5-sonnet-latest",
  "max_tokens": 64,
  "messages": [{"role": "user", "content": "hello"}]
}"#)?;

let openai = translate_request(&req, &cfg)?;
assert_eq!(openai.model, "gpt-4o");
# Ok::<(), Box<dyn std::error::Error>>(())
```
