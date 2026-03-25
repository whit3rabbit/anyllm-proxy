# PRD: Phase 5 - Tool Calling Translation

## Introduction

Implement the conversation history translation for tool calling: converting Anthropic `tool_use` content blocks and `tool_result` content blocks into OpenAI's `tool_calls` in assistant messages and `tool` role messages. This phase handles the structural mismatch where Anthropic uses content blocks within messages while OpenAI uses separate message roles and a JSON-string arguments format.

## Goals

- Translate assistant `tool_use` content blocks to OpenAI `tool_calls` in assistant messages
- Translate user `tool_result` content blocks to OpenAI `tool` role messages
- Handle the JSON object (Anthropic `input`) vs JSON string (OpenAI `arguments`) conversion
- Preserve tool call IDs across the round trip without server-side state
- Support multi-turn conversations with interleaved tool calls

## User Stories

### US-001: Stateless ID bridge
**Description:** As a developer, I need tool call IDs to pass through unchanged so the proxy requires no session storage for tool call tracking.

**Acceptance Criteria:**
- [ ] OpenAI `tool_call.id` is used directly as Anthropic `tool_use.id`
- [ ] Client-provided `tool_result.tool_use_id` is used directly as OpenAI `tool_call_id`
- [ ] No ID rewriting, mapping table, or server-side storage
- [ ] Test: ID `"call_abc123"` survives a full round trip

### US-002: Conversation history walker
**Description:** As a developer, I need the message mapper to walk Anthropic conversation history and correctly split content blocks into OpenAI message structures.

**Acceptance Criteria:**
- [ ] Anthropic assistant message with `tool_use` blocks -> OpenAI assistant message with `tool_calls` array
- [ ] Anthropic assistant message with mixed text + `tool_use` -> OpenAI assistant message with `content` (text) AND `tool_calls`
- [ ] Anthropic user message with `tool_result` blocks -> one OpenAI `tool` role message per tool_result
- [ ] Anthropic user message with mixed text + `tool_result` -> OpenAI text user message + separate tool messages
- [ ] Multi-turn: correctly handles sequences of user -> assistant(tool_use) -> user(tool_result) -> assistant(text)
- [ ] Test: multi-turn conversation with 2+ tool calls produces correct OpenAI message sequence

### US-003: JSON object to JSON string conversion
**Description:** As a developer, I need Anthropic `tool_use.input` (JSON object) converted to OpenAI `arguments` (JSON string) and vice versa, handling edge cases.

**Acceptance Criteria:**
- [ ] Anthropic `input: {"ticker": "AAPL"}` -> OpenAI `arguments: "{\"ticker\":\"AAPL\"}"`
- [ ] OpenAI `arguments: "{\"ticker\":\"AAPL\"}"` -> Anthropic `input: {"ticker": "AAPL"}`
- [ ] Invalid JSON in OpenAI `arguments` -> preserve as-is in a best-effort manner (log warning)
- [ ] Empty input `{}` -> `"{}"` and back
- [ ] Test: valid JSON, empty JSON, nested objects

### US-004: Tool result content translation
**Description:** As a developer, I need tool result content blocks translated to OpenAI tool message content.

**Acceptance Criteria:**
- [ ] Anthropic `tool_result` with string content -> OpenAI tool message with string content
- [ ] Anthropic `tool_result` with array of content blocks -> concatenated text content in OpenAI tool message
- [ ] Anthropic `tool_result` with `is_error: true` -> OpenAI tool message content (error info preserved as text)
- [ ] Test: string result, structured result, error result

### US-005: Response tool call translation (OpenAI -> Anthropic)
**Description:** As a developer, I need OpenAI assistant responses containing `tool_calls` translated back to Anthropic `tool_use` content blocks.

**Acceptance Criteria:**
- [ ] OpenAI `tool_calls` array -> Anthropic `content` array with `tool_use` blocks
- [ ] OpenAI `arguments` (JSON string) -> Anthropic `input` (JSON object)
- [ ] Mixed content + tool_calls -> Anthropic content array with text block followed by tool_use blocks
- [ ] `finish_reason: "tool_calls"` -> Anthropic `stop_reason: "tool_use"`
- [ ] Test: single tool call, multiple parallel tool calls, mixed text + tool calls

## Functional Requirements

- FR-1: Tool call ID is passed through without modification (Anthropic `tool_use.id` = OpenAI `tool_call.id`)
- FR-2: Conversation walker processes messages in order, splitting/combining as needed
- FR-3: JSON string/object conversion uses `serde_json::to_string` / `serde_json::from_str` with defensive error handling
- FR-4: All functions remain pure (no IO, no state beyond the conversation being translated)
- FR-5: Tool definitions already handled by Phase 4; this phase focuses on tool call/result instances

## Non-Goals

- No streaming tool input deltas (Phase 7)
- No tool definition translation (already in Phase 4)
- No parallel tool use policy enforcement
- No tool result validation against tool schemas

## Technical Considerations

- The conversation walker must handle messages where a single Anthropic message contains both text and tool_use/tool_result blocks, splitting them into multiple OpenAI messages
- Order matters: OpenAI tool messages must appear after the assistant message that generated the tool_calls
- OpenAI `arguments` can be partial or malformed JSON (especially in streaming, but handle defensively even in non-streaming)
- Consider using `serde_json::from_str` with a fallback that wraps malformed JSON in an error object

## Success Metrics

- Golden fixture test: multi-turn tool conversation from PLAN.md lines 273-386 translates correctly in both directions
- Round-trip: Anthropic tool conversation -> OpenAI -> back to Anthropic produces equivalent structure
- `cargo test -p anyllm_translate` passes with all new tests
