# PRD: Phase 2 - Anthropic Domain Types

## Introduction

Define the Rust types that model the Anthropic Messages API surface: request, response, content blocks, errors, and headers. These are the "frontend" types the proxy accepts from clients. They must serialize/deserialize correctly against real Anthropic API payloads.

## Goals

- Model `AnthropicMessageCreateRequest` and `AnthropicMessageCreateResponse` as strongly typed Rust structs
- Support all content block types: text, image, document, tool_use, tool_result
- Define error types matching Anthropic's `{type: "error", error: {type, message}}` shape
- Parse Anthropic-specific headers: `x-api-key`, `anthropic-version`, `anthropic-beta`
- Validate with round-trip serde tests and fixture-based golden tests

## User Stories

### US-001: Message request types
**Description:** As a developer working on translation logic, I need typed structs for Anthropic message create requests so I can pattern-match on fields instead of working with raw JSON.

**Acceptance Criteria:**
- [ ] `AnthropicMessageCreateRequest` struct with fields: `model`, `max_tokens`, `messages`, `system`, `temperature`, `top_p`, `stop_sequences`, `tools`, `tool_choice`, `metadata`, `stream`
- [ ] `#[serde(flatten)] extra: Map<String, Value>` for forward compatibility
- [ ] `AnthropicInputMessage` with `role` (user/assistant) and `content` (string or array of content blocks)
- [ ] `AnthropicSystem` supporting both string and array-of-blocks forms
- [ ] Deserializes from PLAN.md example JSON (lines 207-214) without error

### US-002: Content block types
**Description:** As a developer, I need typed enums for every Anthropic content block so tool_use, images, and documents are first-class types, not untyped JSON.

**Acceptance Criteria:**
- [ ] `AnthropicContentBlock` enum with variants: `Text`, `Image`, `Document`, `ToolUse`, `ToolResult`
- [ ] `Text` variant: `{type: "text", text: String}`
- [ ] `Image` variant: `{type: "image", source: ImageSource}` with base64 and url source types
- [ ] `Document` variant: `{type: "document", source: DocumentSource}` with base64 PDF support
- [ ] `ToolUse` variant: `{type: "tool_use", id: String, name: String, input: Value}`
- [ ] `ToolResult` variant: `{type: "tool_result", tool_use_id: String, content: ToolResultContent}`
- [ ] Serde tags use `#[serde(tag = "type")]` for correct JSON representation

### US-003: Message response types
**Description:** As a developer, I need response types so the proxy can construct valid Anthropic-shaped responses from translated OpenAI data.

**Acceptance Criteria:**
- [ ] `AnthropicMessageCreateResponse` with fields: `id`, `type` (always "message"), `role`, `content` (Vec of content blocks), `model`, `stop_reason`, `stop_sequence`, `usage`
- [ ] `AnthropicUsage` struct: `input_tokens`, `output_tokens`, optional `cache_creation_input_tokens`, `cache_read_input_tokens`
- [ ] `StopReason` enum: `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`
- [ ] Serializes to match PLAN.md example JSON (lines 220-228)

### US-004: Error types
**Description:** As a developer, I need Anthropic error types so the proxy returns errors in the exact shape Anthropic clients expect.

**Acceptance Criteria:**
- [ ] `AnthropicError` struct: `{type: "error", error: AnthropicErrorBody}`
- [ ] `AnthropicErrorBody`: `{type: String, message: String}`
- [ ] Error type constants: `invalid_request_error`, `authentication_error`, `permission_error`, `not_found_error`, `request_too_large`, `rate_limit_error`, `api_error`, `overloaded_error`
- [ ] HTTP status code mapping function: error type -> status code (400, 401, 403, 404, 413, 429, 500, 529)

### US-005: Header parsing
**Description:** As a developer working on middleware, I need utilities to extract and validate Anthropic-specific headers from incoming requests.

**Acceptance Criteria:**
- [ ] Function to extract `x-api-key` from request headers, returning error if missing
- [ ] Function to extract `anthropic-version` header
- [ ] Function to extract optional `anthropic-beta` header (comma-separated list)
- [ ] Unit tests for present, missing, and malformed header cases

### US-006: Serde round-trip and fixture tests
**Description:** As a developer, I need confidence that types serialize and deserialize correctly against real API shapes, not just compile.

**Acceptance Criteria:**
- [ ] Golden fixture files in `fixtures/anthropic/`: `messages_basic.json`, `messages_tool_use.json`, `messages_image.json`, `messages_document.json`
- [ ] Tests that deserialize each fixture into typed structs and re-serialize, comparing output
- [ ] Test that malformed requests (missing `max_tokens`, invalid `role`) are rejected by serde
- [ ] `cargo test -p anthropic_openai_translate` passes

## Functional Requirements

- FR-1: All Anthropic content block types defined in PLAN.md lines 64-76 are representable as typed Rust structs
- FR-2: Deserialization uses `#[serde(tag = "type")]` for content blocks so `{"type": "text", "text": "..."}` maps to the `Text` variant
- FR-3: Unknown fields are captured in `extra` maps, not rejected, for forward compatibility
- FR-4: Error types can be constructed from an error type string and message, producing valid JSON matching Anthropic's documented shape
- FR-5: `AnthropicMetadata` includes `user_id` field

## Non-Goals

- No streaming event types (Phase 7)
- No translation to/from OpenAI types (Phase 4)
- No HTTP handling or middleware (Phase 6)
- No beta Files or Skills API types

## Technical Considerations

- Use `#[serde(rename_all = "snake_case")]` where field names match
- Use `#[serde(skip_serializing_if = "Option::is_none")]` to keep serialized output clean
- `tool_use.input` is `serde_json::Value` (arbitrary JSON object), not a typed struct
- `tool_result.content` can be a string or array of content blocks; model with an enum

## Success Metrics

- All fixture files deserialize without error
- Round-trip (deserialize then serialize) produces semantically identical JSON
- `cargo test -p anthropic_openai_translate` passes with zero failures
