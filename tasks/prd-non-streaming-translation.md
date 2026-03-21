# PRD: Phase 4 - Non-Streaming Message Translation

## Introduction

Implement the core translation logic that converts Anthropic Messages API requests into OpenAI Chat Completions requests, and converts OpenAI responses back into Anthropic response shapes. This is pure `fn(A) -> B` logic with no IO, covering message mapping, tool definition translation, usage field mapping, error mapping, and stop reason mapping.

## Goals

- Translate Anthropic request fields to OpenAI Chat Completions fields
- Translate OpenAI Chat Completions responses back to Anthropic response shapes
- Map system prompts, roles, sampling params, stop reasons, and usage correctly
- Handle edge cases: temperature clamping, stop sequence truncation, missing fields
- All translation logic is stateless, IO-free, and testable without mocks

## User Stories

### US-001: Message and role mapping
**Description:** As a developer, I need a function that converts Anthropic messages (with system as top-level field) into OpenAI messages (with system/developer as a role in the messages array).

**Acceptance Criteria:**
- [ ] `map_anthropic_to_openai_request(req: &AnthropicMessageCreateRequest) -> OpenAIChatCompletionRequest`
- [ ] Anthropic `system` string -> OpenAI `developer` role message at position 0
- [ ] Anthropic `system` array of text blocks -> concatenated into single `developer` message
- [ ] Anthropic user/assistant messages -> OpenAI user/assistant messages with correct content
- [ ] Model name passed through (no mapping in this phase)
- [ ] Test: basic text request with system prompt produces correct OpenAI shape

### US-002: Sampling parameter translation
**Description:** As a developer, I need sampling parameters translated with proper clamping and defaults.

**Acceptance Criteria:**
- [ ] `temperature`: passed through; values > 1.0 clamped to 1.0 with a warning (Anthropic max is 1.0, OpenAI allows up to 2.0)
- [ ] `top_p`: passed through directly
- [ ] `max_tokens`: mapped directly to OpenAI `max_tokens`
- [ ] `stop_sequences`: mapped to OpenAI `stop`; truncated to first 4 entries (OpenAI limit) with warning
- [ ] Test: temperature 0.5 passes through, temperature 1.5 clamps to 1.0

### US-003: Tool definition mapping
**Description:** As a developer, I need Anthropic tool definitions translated to OpenAI function tool definitions.

**Acceptance Criteria:**
- [ ] Anthropic `tools[].input_schema` -> OpenAI `tools[].function.parameters`
- [ ] Anthropic `tools[].name` -> OpenAI `tools[].function.name`
- [ ] Anthropic `tools[].description` -> OpenAI `tools[].function.description`
- [ ] OpenAI tool wrapper `{type: "function", function: {...}}` added
- [ ] `tool_choice` mapping: Anthropic `any` -> OpenAI `auto`, `none` -> `none`, `{type: "tool", name: X}` -> `{type: "function", function: {name: X}}`
- [ ] Test: single tool def, multiple tool defs, tool_choice variants

### US-004: Response translation (OpenAI -> Anthropic)
**Description:** As a developer, I need a function that converts an OpenAI Chat Completions response into an Anthropic Messages response.

**Acceptance Criteria:**
- [ ] `map_openai_to_anthropic_response(resp: &OpenAIChatCompletionResponse, model: &str) -> AnthropicMessageCreateResponse`
- [ ] OpenAI `choices[0].message.content` -> Anthropic `content: [{type: "text", text: ...}]`
- [ ] Generate Anthropic-style `id` (e.g., `msg_` + uuid)
- [ ] `type` always set to `"message"`, `role` always `"assistant"`
- [ ] Test: basic text response produces valid Anthropic shape

### US-005: Usage field mapping
**Description:** As a developer, I need token usage fields translated between the two APIs.

**Acceptance Criteria:**
- [ ] OpenAI `prompt_tokens` -> Anthropic `input_tokens`
- [ ] OpenAI `completion_tokens` -> Anthropic `output_tokens`
- [ ] Cache fields default to 0 or absent
- [ ] Test: usage fields map correctly

### US-006: Stop reason mapping
**Description:** As a developer, I need finish/stop reasons mapped between OpenAI and Anthropic conventions.

**Acceptance Criteria:**
- [ ] OpenAI `stop` -> Anthropic `end_turn`
- [ ] OpenAI `length` -> Anthropic `max_tokens`
- [ ] OpenAI `tool_calls` -> Anthropic `tool_use`
- [ ] OpenAI `content_filter` -> Anthropic `end_turn` (best effort, no exact equivalent)
- [ ] Null/missing finish_reason -> Anthropic `end_turn` (default)
- [ ] Test: each mapping case

### US-007: Error status code mapping
**Description:** As a developer, I need OpenAI HTTP error status codes translated to Anthropic error types.

**Acceptance Criteria:**
- [ ] OpenAI 400 -> Anthropic `invalid_request_error` (400)
- [ ] OpenAI 401 -> Anthropic `authentication_error` (401)
- [ ] OpenAI 403 -> Anthropic `permission_error` (403)
- [ ] OpenAI 404 -> Anthropic `not_found_error` (404)
- [ ] OpenAI 429 -> Anthropic `rate_limit_error` (429)
- [ ] OpenAI 500/502/503 -> Anthropic `api_error` (500)
- [ ] Test: each status code maps correctly

## Functional Requirements

- FR-1: `message_map` module converts between message formats bidirectionally
- FR-2: `tools_map` module converts tool definitions and tool_choice
- FR-3: `usage_map` module converts token usage fields
- FR-4: `errors_map` module converts HTTP status codes and error shapes
- FR-5: All mapping functions are pure (no IO, no state, no async)
- FR-6: Comprehensive field mapping table from PLAN.md lines 964-977 is implemented

## Non-Goals

- No tool call/result translation in conversation history (Phase 5)
- No streaming translation (Phase 7)
- No HTTP requests or proxy wiring (Phase 6)
- No model name mapping (configuration concern, Phase 6)

## Technical Considerations

- Functions take references and return owned types to avoid lifetime complexity
- Use `serde_json::Value` for passthrough of unknown fields
- Temperature clamping should log a warning (return clamped value + optional warning)
- Stop sequence truncation should be documented in output (first 4 of N used)

## Success Metrics

- All mapping functions have corresponding unit tests
- Golden fixture tests: load Anthropic fixture, translate to OpenAI, compare against OpenAI fixture
- `cargo test -p anthropic_openai_translate` passes with all new tests green
