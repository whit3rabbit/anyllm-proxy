# PRD: Phase 7 - Streaming SSE Translation

## Introduction

Implement streaming support for `POST /v1/messages` with `stream: true`. The proxy must parse OpenAI's streaming chunks (Chat Completions delta format), translate them into Anthropic's SSE event sequence (`message_start` -> `content_block_start` -> `content_block_delta` -> `content_block_stop` -> `message_delta` -> `message_stop`), and deliver them to the client. This is the highest-complexity translation in the project.

## Goals

- Parse OpenAI Chat Completions streaming chunks (`chat.completion.chunk` + `data: [DONE]`)
- Emit Anthropic SSE events in the correct documented order
- Implement a streaming state machine that tracks content blocks and accumulates usage
- Handle backpressure via bounded channels
- Handle client disconnect gracefully (abort upstream)
- Support tool call streaming (`input_json_delta`)

## User Stories

### US-001: Anthropic SSE emitter
**Description:** As a developer, I need a component that emits properly formatted Anthropic SSE events so streaming clients receive the exact event shapes they expect.

**Acceptance Criteria:**
- [ ] Emits `event: message_start` with initial message skeleton (empty content, null stop_reason, initial usage)
- [ ] Emits `event: content_block_start` with block index and type
- [ ] Emits `event: content_block_delta` with `text_delta` or `input_json_delta`
- [ ] Emits `event: content_block_stop` when a content block is complete
- [ ] Emits `event: message_delta` with `stop_reason` and final `usage`
- [ ] Emits `event: message_stop` as the final event
- [ ] Each event is formatted as `event: {type}\ndata: {json}\n\n`
- [ ] Test: emit a complete event sequence, verify output matches PLAN.md lines 392-408

### US-002: OpenAI Chat Completions chunk parser
**Description:** As a developer, I need a parser that reads OpenAI SSE stream lines and produces typed chunk objects.

**Acceptance Criteria:**
- [ ] Parses `data: {json}` lines into `OpenAIChatCompletionChunk` structs
- [ ] Handles `data: [DONE]` as stream termination signal
- [ ] `OpenAIChatCompletionChunk` has: `id`, `object`, `model`, `choices` (with `delta` and `finish_reason`), optional `usage`
- [ ] `Delta` struct: optional `role`, `content`, `tool_calls`
- [ ] Ignores empty lines and comment lines (`:` prefix)
- [ ] Test: parse a sequence of chunk lines, verify extracted deltas

### US-003: Streaming state machine
**Description:** As a developer, I need a state machine in `streaming_map.rs` that transforms a sequence of OpenAI chunks into a sequence of Anthropic SSE events, maintaining the correct block tracking.

**Acceptance Criteria:**
- [ ] Tracks current content block index (starts at 0)
- [ ] On first text delta: emits `content_block_start(index=0, type=text)` then `content_block_delta`
- [ ] On subsequent text deltas: emits `content_block_delta` only
- [ ] On tool call delta: starts new content block for each tool call, emits `content_block_start(type=tool_use)` then `input_json_delta` deltas
- [ ] On finish_reason received: emits `content_block_stop` for current block, then `message_delta` with mapped stop reason
- [ ] On stream end: emits `message_stop`
- [ ] Accumulates usage from final usage chunk if `stream_options.include_usage` was set
- [ ] Test: text-only stream, tool call stream, mixed text+tool stream

### US-004: Backpressure and bounded channel
**Description:** As a developer, I need the upstream reader and downstream SSE writer connected by a bounded channel so slow clients don't cause unbounded memory growth.

**Acceptance Criteria:**
- [ ] Bounded `mpsc` channel (capacity ~64 events) between upstream reader task and SSE response stream
- [ ] If channel is full, upstream reader awaits (backpressure)
- [ ] SSE response uses `axum::response::Sse` with `ReceiverStream`
- [ ] Test: verify bounded channel does not drop events under normal conditions

### US-005: Client disconnect handling
**Description:** As an operator, I need the proxy to stop reading from OpenAI when a client disconnects, freeing resources.

**Acceptance Criteria:**
- [ ] When the downstream SSE connection drops, the channel receiver is dropped
- [ ] The upstream reader task detects sender failure and aborts
- [ ] Upstream HTTP connection to OpenAI is closed/dropped
- [ ] No orphaned tasks or connections after client disconnect
- [ ] Test: simulate client disconnect mid-stream, verify upstream abort

### US-006: Streaming route integration
**Description:** As a client, I need `POST /v1/messages` with `stream: true` to return an SSE response instead of a JSON response.

**Acceptance Criteria:**
- [ ] Request with `stream: true` triggers streaming path
- [ ] Response has `Content-Type: text/event-stream`
- [ ] Response includes `Cache-Control: no-cache`
- [ ] OpenAI request is sent with `stream: true` and `stream_options: {include_usage: true}`
- [ ] Events arrive incrementally (not buffered until completion)
- [ ] Test: integration test with mocked streaming OpenAI backend, verify Anthropic SSE event sequence

### US-007: Golden fixture tests
**Description:** As a developer, I need fixture-based tests comparing full SSE transcripts to catch regressions in streaming translation.

**Acceptance Criteria:**
- [ ] Fixture file: `fixtures/openai/chat_completion_stream_text.txt` (OpenAI SSE transcript)
- [ ] Fixture file: `fixtures/anthropic/stream_message_text.txt` (expected Anthropic SSE transcript)
- [ ] Test feeds OpenAI fixture through state machine, compares output to Anthropic fixture
- [ ] IDs and timestamps are normalized before comparison
- [ ] At least fixtures for: text streaming, tool call streaming

## Functional Requirements

- FR-1: Streaming state machine processes OpenAI chunks one at a time, producing zero or more Anthropic events per chunk
- FR-2: Event ordering strictly follows: `message_start` -> (`content_block_start` -> `content_block_delta`* -> `content_block_stop`)+ -> `message_delta` -> `message_stop`
- FR-3: `stream_options: {include_usage: true}` is always set on OpenAI requests to capture final usage
- FR-4: Bounded channel provides backpressure, not event dropping
- FR-5: Client disconnect triggers upstream cleanup within 1 second

## Non-Goals

- No OpenAI Responses API streaming (Chat Completions only for now)
- No WebSocket support
- No streaming obfuscation/side-channel mitigation
- No partial response buffering for content moderation

## Technical Considerations

- Use `tokio::sync::mpsc` for the bounded channel
- Use `axum::response::Sse` with `futures::stream::Stream` for SSE delivery
- The state machine should be a struct with methods, not a closure chain, for testability
- OpenAI may send multiple tool call deltas interleaved by index; track by tool call index
- The usage chunk may not arrive if the stream is interrupted; handle gracefully

## Success Metrics

- Golden fixture tests pass for text and tool call streaming
- Integration test: full streaming round trip with mock backend
- No memory growth under sustained streaming load (bounded channel enforced)
- `cargo test` passes for both crates
