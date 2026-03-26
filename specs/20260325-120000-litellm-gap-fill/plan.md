# Implementation Plan: LiteLLM Gap Fill + Rust Client Library

**Branch**: `20260325-120000-litellm-gap-fill` | **Date**: 2026-03-25 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/20260325-120000-litellm-gap-fill/spec.md`

## Summary

Close the highest-value feature gaps between anyllm-proxy and LiteLLM: accept OpenAI Chat Completions input, add AWS Bedrock and Azure OpenAI backends, implement virtual key management with per-key rate limiting, improve the Rust client library, and add optional OpenTelemetry export. Research is complete (see [research.md](./research.md)).

## Technical Context

**Language/Version**: Rust stable, Cargo workspace (3 crates)
**Primary Dependencies**: axum, reqwest, tokio, serde, tracing, rusqlite, sha2; NEW: aws-sigv4, aws-credential-types, dashmap; OPTIONAL: opentelemetry 0.31, tracing-opentelemetry 0.32
**Storage**: SQLite (existing admin DB, extended with `virtual_api_key` table)
**Testing**: `cargo test` (~480 existing tests); new unit + integration tests per requirement
**Target Platform**: Linux/macOS server, single static binary
**Project Type**: Web service (HTTP proxy)
**Performance Goals**: Existing 100 concurrent request limit; virtual key auth adds one DashMap lookup per request
**Constraints**: Source files under 400 lines (excluding tests); translator crate must remain IO-free
**Scale/Scope**: 7 requirements (5 Tier 1, 2 Tier 2); ~15 new/modified files across 3 crates

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Security First | PASS | SHA-256 key hashing (existing pattern), SigV4 via audited crate, no secrets in code/logs |
| II. Test Coverage | PASS | Each requirement has acceptance tests defined in spec; TDD approach |
| III. File Size Discipline | PASS | New modules scoped to single concerns; reverse streaming translator is a new file |
| IV. Code Quality | PASS | `cargo clippy -- -D warnings`, `cargo fmt --check` required per quality gates |
| V. Minimal and Correct Changes | PASS | Reuses existing types and patterns; Azure reuses OpenAI client code |
| Dependency Policy | REVIEW NEEDED | 3 new prod deps: `aws-sigv4`, `aws-credential-types`, `dashmap`. Justified: SigV4 cannot be safely hand-rolled; DashMap replaces what would be `RwLock<HashMap>` on the hot auth path. OTEL deps are optional (feature-gated). |

**Post-design re-check**: All file counts estimated under 400 lines. `reverse_streaming_map.rs` is the largest new file (~250 lines estimated). Constitution compliant.

## Project Structure

### Documentation (this feature)

```text
specs/20260325-120000-litellm-gap-fill/
├── plan.md              # This file
├── research.md          # Phase 0 output (complete)
├── data-model.md        # Phase 1 output (complete)
├── quickstart.md        # Phase 1 output (complete)
├── contracts/
│   ├── chat-completions.md   # POST /v1/chat/completions contract
│   └── admin-keys.md         # Virtual key admin API contract
└── tasks.md             # Phase 2 output (NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/translator/src/
├── mapping/
│   ├── message_map.rs              # MODIFIED: add openai_to_anthropic_request, anthropic_to_openai_response
│   ├── reverse_streaming_map.rs    # NEW: ReverseStreamingTranslator (Anthropic SSE -> OpenAI chunks)
│   ├── mod.rs                      # MODIFIED: pub mod reverse_streaming_map
│   └── [existing files unchanged]
├── translate.rs                    # MODIFIED: add reverse translation convenience wrappers
└── lib.rs                          # MODIFIED: re-exports

crates/proxy/src/
├── config/
│   └── mod.rs                      # MODIFIED: BackendKind::Bedrock, BackendKind::AzureOpenAI, env var parsing
├── backend/
│   ├── mod.rs                      # MODIFIED: BackendClient::Bedrock, BackendClient::AzureOpenAI variants
│   ├── openai_client.rs            # MODIFIED: Azure URL construction + api-key header
│   ├── bedrock_client.rs           # NEW: SigV4-signed reqwest client, event stream decoder
│   └── [existing files unchanged]
├── server/
│   ├── routes.rs                   # MODIFIED: register POST /v1/chat/completions
│   ├── chat_completions.rs         # NEW: handler for OpenAI-format input
│   └── [existing files unchanged]
├── admin/
│   ├── routes.rs                   # MODIFIED: add key management endpoints
│   ├── db.rs                       # MODIFIED: virtual_api_key table DDL + CRUD
│   └── keys.rs                     # NEW: key generation, hashing, validation logic
├── middleware/
│   └── auth.rs                     # MODIFIED: extend to check DashMap virtual keys
├── otel.rs                         # NEW: OpenTelemetry init (behind #[cfg(feature = "otel")])
└── main.rs                         # MODIFIED: OTEL guard, DashMap init from DB

crates/client/src/
├── client.rs                       # MODIFIED: ClientBuilder, Stream return type
├── lib.rs                          # MODIFIED: re-exports, version 0.2.0
└── tools.rs                        # NEW: ToolBuilder, ToolChoiceBuilder helpers
```

**Structure Decision**: Existing 3-crate workspace is preserved. No new crates. New functionality distributed across existing module boundaries. The translator crate remains IO-free.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `dashmap` new dependency | Hot-path auth check for virtual keys needs concurrent reads without global lock | `RwLock<HashMap>` serializes all reads during any write; unacceptable for auth middleware on every request |
| `aws-sigv4` + `aws-credential-types` new dependencies | SigV4 request signing requires HMAC chain, canonical request construction, and session token handling | Manual implementation (~150 lines) is error-prone and unaudited; the official crate is ~22K SLoC and well-tested |
| Bedrock event stream decoder | AWS streaming uses binary framing, not SSE | `aws-smithy-eventstream` is already a transitive dep of `aws-sigv4`; alternatively a ~80-line manual parser fits in one file |

## Implementation Phases

### Phase A: Reverse Translation (R1 prerequisite)
1. `openai_to_anthropic_request` in `mapping/message_map.rs`
2. `anthropic_to_openai_response` in `mapping/message_map.rs`
3. `ReverseStreamingTranslator` in `mapping/reverse_streaming_map.rs`
4. Unit tests for all new mapping functions
5. Convenience wrappers in `translate.rs`

### Phase B: Chat Completions Endpoint (R1)
1. `server/chat_completions.rs` handler (non-streaming + streaming)
2. Route registration in `routes.rs`
3. Integration tests (non-streaming, streaming, tool calls, error cases)

### Phase C: Azure Backend (R3)
1. Config parsing: `BackendKind::AzureOpenAI`, env vars
2. URL construction in `openai_client.rs` (reuse existing client)
3. `api-key` auth header variant
4. Integration test (`#[ignore]`, requires Azure credentials)

### Phase D: Bedrock Backend (R2)
1. `bedrock_client.rs`: SigV4 signing with `aws-sigv4`
2. Non-streaming `InvokeModel` path
3. Event stream binary decoder for streaming
4. Config parsing: `BackendKind::Bedrock`, env vars
5. Integration test (`#[ignore]`, requires AWS credentials)

### Phase E: Virtual Key Management (R4)
1. SQLite schema in `admin/db.rs`
2. Key generation and hashing in `admin/keys.rs`
3. Admin API endpoints in `admin/routes.rs`
4. DashMap cache in `SharedState`, loaded from DB on startup
5. Auth middleware extension to check virtual keys
6. Unit + integration tests

### Phase F: Per-Key Rate Limiting (R7, depends on E)
1. `RateLimitState` with sliding window in `admin/keys.rs`
2. RPM/TPM enforcement in auth middleware
3. HTTP 429 + `retry-after` header on limit exceeded
4. Unit tests for window behavior

### Phase G: Client Library (R5)
1. `ClientBuilder` in `client/client.rs`
2. `Stream` return type for SSE
3. `ToolBuilder` in `client/tools.rs`
4. Rustdoc examples on all public types
5. Version bump to 0.2.0

### Phase H: OpenTelemetry (R6)
1. Feature-gated deps in `Cargo.toml`
2. `otel.rs` initialization module
3. `OpenTelemetryLayer` integration in `main.rs`
4. Span attributes for request ID, model, latency, token counts
5. Manual verification with local OTEL collector

### Phase Order / Dependencies

```
A -> B (reverse translation before endpoint)
C (independent, can parallel with A/B)
D (independent, can parallel with A/B/C)
E -> F (virtual keys before rate limiting)
G (independent)
H (independent)
```

Phases A+B are the critical path (highest user value). C and D can proceed in parallel once A is done.
