---
title: "Types"
description: "Reference definitions for the most important public Rust types exported by the anyllm-proxy workspace."
---

This workspace does not publish TypeScript interfaces. The public surface is Rust, so this page collects the Rust types that most directly shape the API contracts across the crates.

## Translation Types

From `crates/translator/src/anthropic/messages.rs`:

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

pub struct MessageResponse {
    pub id: String,
    pub response_type: String,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<StopReason>,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
    pub created: Option<u64>,
}
```

These are the outer Anthropic-facing contracts used by both `anyllm_translate` and `anyllm_client`. The `extra` field on `MessageCreateRequest` is important because it preserves forward compatibility with new Anthropic fields without forcing immediate crate changes.

## OpenAI-Compatible Types

From `crates/translator/src/openai/chat_completions.rs`:

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

pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<ChatUsage>,
    pub created: Option<u64>,
    pub system_fingerprint: Option<String>,
    pub service_tier: Option<String>,
}
```

The proxy uses these when it talks to OpenAI-compatible providers, even if the public endpoint presented to the caller is Anthropic-shaped.

## Catalog Types

From `crates/providers/src/provider.rs` and `crates/providers/src/model.rs`:

```rust
pub struct ProviderDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub protocol: ProviderProtocol,
    pub auth: AuthKind,
    pub status: ProviderStatus,
    pub env_vars: &'static [&'static str],
    pub litellm_prefix: &'static str,
    pub capabilities: ProviderCapabilities,
}

pub struct ModelDef {
    pub id: &'static str,
    pub provider_id: &'static str,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub capabilities: ModelCapabilities,
    pub status: ModelStatus,
}
```

These are compile-time metadata records, not runtime clients. They are why config parsing can accept provider ids like `groq`, `openrouter`, or `zai` and still know which auth variables and protocols to use.

## Batch Types

From `crates/batch_engine/src/job.rs`:

```rust
pub struct BatchSubmission {
    pub items: Vec<SubmissionItem>,
    pub execution_mode: ExecutionMode,
    pub input_file_id: String,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub priority: u8,
}

pub struct SubmissionItem {
    pub custom_id: String,
    pub model: String,
    pub body: serde_json::Value,
    pub source_format: SourceFormat,
}

pub struct BatchJob {
    pub id: BatchId,
    pub status: BatchStatus,
    pub execution_mode: ExecutionMode,
    pub priority: u8,
    pub key_id: Option<i64>,
    pub webhook_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub request_counts: RequestCounts,
    pub input_file_id: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub expires_at: String,
}
```

These types separate submission intent from durable job state. The engine constructs `BatchJob` and `BatchItem` records from `BatchSubmission`, which is why validation happens before enqueueing rather than lazily inside workers.

## Runtime Utility Types

From `crates/proxy/src/env_parser.rs`:

```rust
pub struct ParseResult {
    pub pairs: Vec<ParsedPair>,
    pub warnings: Vec<EnvWarning>,
    pub hard_errors: Vec<String>,
}
```

This type is the reason env imports can be previewed safely. The parser returns accepted pairs, soft warnings, and fatal errors separately, so callers can abort without mutating process state or writing partially valid config.
