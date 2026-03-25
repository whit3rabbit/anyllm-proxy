# PRD: Phase 1 - Project Scaffolding

## Introduction

Set up the Cargo workspace, dependencies, and directory structure for the Anthropic-to-OpenAI translation proxy. This is the foundation everything else builds on. Two crates: `translator` (pure library, no IO) and `proxy` (axum binary).

## Goals

- Establish a working Cargo workspace with two crates
- Pin initial dependency versions for tokio, axum, reqwest, serde, serde_json
- Create the directory tree matching PLAN.md lines 606-665
- Verify the build compiles and a health endpoint responds 200

## User Stories

### US-001: Create Cargo workspace
**Description:** As a developer, I need a workspace root `Cargo.toml` that declares `translator` and `proxy` as members so I can build both crates with a single command.

**Acceptance Criteria:**
- [ ] `Cargo.toml` at repo root with `[workspace]` declaring `crates/translator` and `crates/proxy`
- [ ] `cargo build` succeeds with no errors
- [ ] `cargo test` runs (even if no tests yet)

### US-002: Scaffold translator crate
**Description:** As a developer, I need the `anyllm_translate` library crate with the module structure defined in PLAN.md so subsequent phases have a place to add types and mapping logic.

**Acceptance Criteria:**
- [ ] `crates/translator/Cargo.toml` with `serde`, `serde_json`, `uuid`, `thiserror` dependencies
- [ ] `src/lib.rs` with module declarations for `anthropic`, `openai`, `mapping`, `util`
- [ ] Subdirectories: `anthropic/`, `openai/`, `mapping/`, `util/` with `mod.rs` stubs
- [ ] `cargo build -p anyllm_translate` succeeds

### US-003: Scaffold proxy crate
**Description:** As a developer, I need the `anyllm_proxy` binary crate with axum server skeleton and a health endpoint so I can verify the server starts.

**Acceptance Criteria:**
- [ ] `crates/proxy/Cargo.toml` with `tokio`, `axum`, `reqwest`, `tracing`, `tracing-subscriber` dependencies
- [ ] `src/main.rs` starts a tokio runtime and binds axum router
- [ ] `src/config.rs` reads `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `LISTEN_PORT` from env
- [ ] `src/server/routes.rs` with `GET /health` returning 200 `{"status":"ok"}`
- [ ] Module stubs for `server/middleware.rs`, `server/sse.rs`, `backend/openai_client.rs`, `metrics/mod.rs`

### US-004: Health endpoint integration test
**Description:** As a developer, I need an integration test proving the server starts and the health endpoint responds, confirming the scaffolding works end to end.

**Acceptance Criteria:**
- [ ] `crates/proxy/tests/` directory with at least one integration test
- [ ] Test starts the server on a random port, hits `GET /health`, asserts 200
- [ ] `cargo test health_endpoint` passes

### US-005: Create fixture directories
**Description:** As a developer, I need `fixtures/anthropic/` and `fixtures/openai/` directories for golden-file testing in later phases.

**Acceptance Criteria:**
- [ ] `fixtures/anthropic/` and `fixtures/openai/` directories exist
- [ ] At least one placeholder `.json` file in each (can be empty object)

## Functional Requirements

- FR-1: Workspace builds with `cargo build` producing no errors or warnings
- FR-2: `cargo test` runs and passes (even with zero test assertions initially)
- FR-3: `cargo run -p anyllm_proxy` starts a server on the configured port
- FR-4: `GET /health` returns HTTP 200 with JSON body `{"status":"ok"}`
- FR-5: Environment variables `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `LISTEN_PORT` are read with sensible defaults

## Non-Goals

- No translation logic
- No OpenAI client calls
- No authentication middleware
- No streaming support
- No CI/CD pipeline (handled separately)

## Technical Considerations

- Use `tokio` with `rt-multi-thread` and `macros` features
- axum 0.7+ for the server framework
- reqwest with `rustls-tls` (avoid openssl dep)
- Keep dependency count minimal; only add what Phase 1 needs

## Success Metrics

- `cargo build` completes in under 60 seconds on a clean build
- `cargo test` exits 0
- Health endpoint responds within 10ms locally
