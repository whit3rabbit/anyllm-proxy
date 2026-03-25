# PRD: Phase 3 - OpenAI Domain Types

## Introduction

Define the Rust types that model the OpenAI Chat Completions and Responses APIs: requests, responses, tool types, errors, and rate limit headers. These are the "backend" types the proxy uses when communicating with OpenAI.

## Goals

- Model `OpenAIChatCompletionRequest` and `OpenAIChatCompletionResponse` structs
- Model `OpenAIResponsesRequest` and `OpenAIResponsesResponse` structs
- Define OpenAI error types and rate limit header parsing
- Define OpenAI tool/function calling types
- Validate with round-trip serde tests and fixture-based golden tests

## User Stories

### US-001: Chat Completions request types
**Description:** As a developer working on the backend client, I need typed structs for OpenAI Chat Completions requests so I can construct valid payloads to send to OpenAI.

**Acceptance Criteria:**
- [ ] `OpenAIChatCompletionRequest` struct with fields: `model`, `messages`, `max_tokens`, `temperature`, `top_p`, `stop`, `tools`, `tool_choice`, `stream`, `stream_options`
- [ ] `#[serde(flatten)] extra` for forward compatibility
- [ ] `OpenAIChatMessage` with roles: `system`, `user`, `assistant`, `developer`, `tool`
- [ ] `OpenAIStop` supporting both single string and string array forms
- [ ] `OpenAIStreamOptions` with `include_usage: bool`
- [ ] Serializes to match PLAN.md example JSON (lines 237-246)

### US-002: Chat Completions response types
**Description:** As a developer, I need typed response structs so the translator can extract content, tool calls, usage, and stop reasons from OpenAI responses.

**Acceptance Criteria:**
- [ ] `OpenAIChatCompletionResponse` with fields: `id`, `object`, `model`, `choices`, `usage`
- [ ] `OpenAIChatChoice` with `index`, `message`, `finish_reason`
- [ ] `OpenAIChatResponseMessage` with `role`, `content` (Option<String>), `tool_calls` (Option<Vec>)
- [ ] `FinishReason` enum: `stop`, `length`, `tool_calls`, `content_filter`
- [ ] `OpenAIUsage` struct: `prompt_tokens`, `completion_tokens`, `total_tokens`
- [ ] Deserializes from PLAN.md example JSON (lines 249-265)

### US-003: OpenAI tool types
**Description:** As a developer, I need typed tool definitions matching OpenAI's function-calling schema so tool translation is type-safe.

**Acceptance Criteria:**
- [ ] `OpenAITool` struct with `type` (always "function") and `function: OpenAIFunction`
- [ ] `OpenAIFunction` with `name`, `description`, `parameters` (Value for JSON Schema)
- [ ] `OpenAIToolCall` with `id`, `type`, `function: OpenAIFunctionCall`
- [ ] `OpenAIFunctionCall` with `name`, `arguments` (String, since OpenAI sends JSON as string)
- [ ] `OpenAIToolChoice` supporting string forms ("auto", "none", "required") and object form `{type, function: {name}}`

### US-004: Responses API types (basic)
**Description:** As a developer, I need basic types for the OpenAI Responses API to support it as an alternative backend, especially for file/document inputs.

**Acceptance Criteria:**
- [ ] `OpenAIResponsesRequest` with `model`, `input` (string or array of input items), `instructions`, `max_output_tokens`, `tools`, `stream`
- [ ] `OpenAIResponsesInputItem` enum supporting text, image, and `input_file` (with `file_data`, `file_id`, `file_url` variants)
- [ ] `OpenAIResponsesResponse` with `id`, `output`, `status`, `usage`
- [ ] Basic serialization/deserialization tests

### US-005: Error types and rate limit headers
**Description:** As a developer, I need OpenAI error types and rate limit header parsing so the proxy can handle upstream failures and map them to Anthropic error shapes.

**Acceptance Criteria:**
- [ ] `OpenAIError` struct matching OpenAI's `{error: {message, type, param, code}}` shape
- [ ] Rate limit header parsing: `x-ratelimit-limit-requests`, `x-ratelimit-remaining-requests`, `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens`, `x-ratelimit-reset-requests`, `x-ratelimit-reset-tokens`
- [ ] Struct `OpenAIRateLimitInfo` holding parsed values
- [ ] Unit tests for error deserialization and header parsing

### US-006: Fixture tests
**Description:** As a developer, I need golden-file tests to confirm OpenAI types match real API shapes.

**Acceptance Criteria:**
- [ ] Fixture files in `fixtures/openai/`: `chat_completion_basic.json`, `chat_completion_tool_call.json`, `responses_basic.json`
- [ ] Tests that deserialize each fixture into typed structs without error
- [ ] Round-trip serialization produces semantically identical JSON
- [ ] `cargo test -p anyllm_translate` passes

## Functional Requirements

- FR-1: All OpenAI Chat Completions fields from PLAN.md lines 78-87 are modeled
- FR-2: All OpenAI Responses fields from PLAN.md lines 89-94 are modeled (basic subset)
- FR-3: `OpenAIChatMessage.content` supports both string and structured content (array of parts with text/image_url)
- FR-4: `OpenAIToolCall.function.arguments` is `String` (not parsed JSON) to match OpenAI's wire format
- FR-5: Error types support deserialization from OpenAI error JSON responses

## Non-Goals

- No streaming chunk types (Phase 7)
- No WebSocket mode types
- No MCP tool types
- No file upload multipart handling

## Technical Considerations

- OpenAI `arguments` is a JSON string that may be malformed; keep as `String`, parse defensively in mapping layer
- `finish_reason` may be null in streaming chunks; use `Option<FinishReason>`
- Content in assistant messages is `Option<String>` (null when tool_calls present)
- Rate limit headers use different reset formats (absolute timestamp vs relative seconds)

## Success Metrics

- All fixture files deserialize without error
- Round-trip serde produces equivalent JSON
- `cargo test -p anyllm_translate` passes
