# PRD: Phase 11 - End-to-End Validation

## Introduction

Validate the complete proxy against the full Anthropic API surface. Run the entire test suite, verify fixtures against current API documentation, check the compatibility contract, and optionally run live API tests against real OpenAI endpoints. This phase is about confidence, not new features.

## Goals

- Full test suite passes: unit, integration, and fixture tests across both crates
- Fixture files validated against current Anthropic and OpenAI API docs
- Compatibility checklist verified (what works, what is approximated, what is rejected)
- End-to-end flows tested: basic text, tool calling, streaming, files, errors
- Optional: live API test against real OpenAI endpoint

## User Stories

### US-001: Full test suite green
**Description:** As a developer preparing for release, I need every test in the project to pass, confirming no regressions across all phases.

**Acceptance Criteria:**
- [ ] `cargo test` passes with zero failures
- [ ] `cargo clippy -- -D warnings` passes with zero warnings
- [ ] `cargo fmt --check` passes
- [ ] No ignored tests without documented reason

### US-002: Fixture validation
**Description:** As a developer, I need fixture files checked against current API documentation to confirm they reflect real API shapes.

**Acceptance Criteria:**
- [ ] Review all `fixtures/anthropic/*.json` against current Anthropic Messages API docs
- [ ] Review all `fixtures/openai/*.json` against current OpenAI Chat Completions docs
- [ ] Update any fixtures where API shapes have changed
- [ ] Document any known fixture deviations with comments

### US-003: Compatibility checklist
**Description:** As an operator, I need a clear compatibility matrix documenting what the proxy supports, what it approximates, and what it rejects.

**Acceptance Criteria:**
- [ ] Basic messages (text): verified working
- [ ] Streaming text: verified working
- [ ] Tool use (function calling): verified working
- [ ] Image content blocks: verified working
- [ ] Document (PDF) content blocks: verified with status noted
- [ ] Token counting endpoint: status documented (approximate or unsupported)
- [ ] Batch endpoint: documented as unsupported
- [ ] Stop reason mapping: all cases documented
- [ ] Checklist written to `docs/compatibility.md`

### US-004: E2E flow tests
**Description:** As a developer, I need integration tests that exercise full request/response flows through the proxy with mocked upstream.

**Acceptance Criteria:**
- [ ] E2E test: basic text message -> response with text content
- [ ] E2E test: message with tools -> response with tool_use -> follow-up with tool_result -> final text
- [ ] E2E test: streaming text message -> correct SSE event sequence
- [ ] E2E test: streaming with tool calls -> correct SSE event sequence
- [ ] E2E test: image content block in request -> correct upstream payload
- [ ] E2E test: upstream 429 -> Anthropic-shaped error (or retry succeeds)
- [ ] E2E test: upstream 500 -> Anthropic-shaped error
- [ ] E2E test: auth failure -> 401

### US-005: Optional live API test
**Description:** As a developer with an OpenAI API key, I optionally want to run tests against the real OpenAI API to catch any mismatch between fixtures and reality.

**Acceptance Criteria:**
- [ ] Live tests gated behind `#[ignore]` attribute or feature flag
- [ ] Requires `OPENAI_API_KEY` env var to run
- [ ] Tests: basic completion, streaming completion, tool call
- [ ] Clearly documented how to run: `cargo test -- --ignored` or `cargo test --features live-tests`
- [ ] Test output includes model used and response validation

## Functional Requirements

- FR-1: `cargo test` with no flags runs all non-live tests
- FR-2: `cargo clippy -- -D warnings` treats all warnings as errors
- FR-3: Compatibility checklist is a markdown file in the repo
- FR-4: E2E tests use the same mocking infrastructure as Phase 6/7 integration tests
- FR-5: Live tests are opt-in and never run in CI by default

## Non-Goals

- No performance benchmarking (separate concern)
- No load testing
- No fuzzing (mentioned in PLAN.md CI section, but separate task)
- No deployment automation

## Technical Considerations

- Use `wiremock` or `httpmock` for E2E mock server
- Live tests should use cheap models (e.g., `gpt-4o-mini`) to minimize cost
- Live tests should have reasonable timeouts (30s per test)
- Consider a test helper that starts the proxy on a random port with a mock backend

## Success Metrics

- `cargo test` exits 0 with all tests passing
- `cargo clippy -- -D warnings` exits 0
- `cargo fmt --check` exits 0
- Compatibility checklist document exists and is accurate
- E2E tests cover all major flows
