# Tasks: LiteLLM Parity — Remaining Gap Closure

**Input**: Design documents from `/specs/001-litellm-parity/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Included per project constitution (Principle II: "Tests are not optional").

**Organization**: Tasks grouped by user story. Tier 1 (US1-US4) before Tier 2 (US5-US9).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story (US1-US9)

---

## Phase 1: Setup

**Purpose**: Add dependencies and create module skeletons

- [X] T001 Add `moka = { version = "0.12", features = ["future"] }` and `serde_yaml = "0.9"` to `[dependencies]` in crates/proxy/Cargo.toml
- [X] T002 [P] Add `redis = { version = "0.27", features = ["tokio-comp", "connection-manager"], optional = true }` and `[features] redis = ["dep:redis"]` to crates/proxy/Cargo.toml
- [X] T003 [P] Add `qdrant-client = { version = "1", optional = true }` and update `[features] qdrant = ["dep:qdrant-client"]` in crates/proxy/Cargo.toml
- [X] T004 Add new env vars (CACHE_TTL_SECS, CACHE_MAX_ENTRIES, REDIS_URL, PROXY_CONFIG) to config loader in crates/proxy/src/config/mod.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Schema migrations and shared type extensions that multiple stories depend on

**CRITICAL**: No user story work can begin until this phase is complete

- [X] T005 Add SQLite schema migration: ALTER TABLE virtual_api_key ADD COLUMN for role, max_budget_usd, budget_duration, period_start, period_spend_usd, total_input_tokens, total_output_tokens in crates/proxy/src/admin/db.rs
- [X] T006 [P] Add batch_file and batch_job CREATE TABLE IF NOT EXISTS statements to crates/proxy/src/admin/db.rs (schema only, per data-model.md)
- [X] T007 Extend VirtualKeyMeta with KeyRole enum (Admin/Developer), BudgetDuration enum (Daily/Monthly), max_budget_usd, budget_duration, period_start, period_spend_usd fields in crates/proxy/src/admin/keys.rs; update VirtualKeyRow and row_to_virtual_key() mapping
- [X] T008 [P] Update insert_virtual_key() and load_active_virtual_keys() in crates/proxy/src/admin/db.rs to read/write new columns (role, max_budget_usd, budget_duration, period_start, period_spend_usd, total_input_tokens, total_output_tokens)
- [X] T009 Create assets/model_pricing.json with pricing for GPT-4o/4o-mini/4/3.5-turbo, Claude 3.5/3/3.7 variants, Gemini 1.5/2.0/2.5 variants, text-embedding-ada-002/text-embedding-3-small/3-large per data-model.md format

**Checkpoint**: Foundation ready. All user stories can now begin.

---

## Phase 3: User Story 1 — Repeated Prompts Return From Cache (Priority: P1) MVP

**Goal**: Identical non-streaming requests return cached responses with `x-anyllm-cache: hit`, no upstream call.

**Independent Test**: Send POST /v1/messages with same body twice; second has `x-anyllm-cache: hit`, upstream call count = 1.

### Tests for User Story 1

- [X] T010 [P] [US1] Unit test: cache_key_for_request() produces same hash for identical canonical fields and different hash when temperature differs in crates/proxy/src/cache/mod.rs
- [X] T011 [P] [US1] Unit test: MemoryCache put/get respects TTL expiry and max capacity in crates/proxy/src/cache/memory.rs
- [X] T012 [US1] Integration test: same request twice returns cache hit on second; different temperature = miss; streaming = bypass; per-request cache_ttl_secs=0 = bypass in crates/proxy/tests/cache.rs

### Implementation for User Story 1

- [X] T013 [P] [US1] Implement CacheBackend trait (get/put async), CacheEntry struct, and cache_key_for_request() (canonical JSON with sorted BTreeMap + SHA-256 + anth:/oai: namespace prefix) in crates/proxy/src/cache/mod.rs
- [X] T014 [US1] Implement MemoryCache struct wrapping moka::future::Cache with configurable TTL and max_entries in crates/proxy/src/cache/memory.rs
- [X] T015 [US1] Wire cache into POST /v1/messages handler: check cache before backend call; on miss, cache successful non-streaming response; set x-anyllm-cache header (hit/miss/bypass) in crates/proxy/src/server/routes.rs
- [X] T016 [US1] Wire cache into POST /v1/chat/completions handler: same logic with oai: namespace prefix in crates/proxy/src/server/chat_completions.rs
- [X] T017 [US1] Parse optional cache_ttl_secs from request body (validate 0-86400; 0 = bypass; negative/over = 400) in crates/proxy/src/cache/mod.rs
- [X] T018 [P] [US1] Implement Redis cache tier (RedisCacheBackend implementing CacheBackend) behind --features redis in crates/proxy/src/cache/redis.rs with unit tests; graceful fallback if REDIS_URL is unreachable

**Checkpoint**: US1 functional. Repeated identical requests return cached responses.

---

## Phase 4: User Story 2 — Transparent Backend Failover (Priority: P1)

**Goal**: When primary backend returns 5xx/429/connection error, proxy silently retries against configured fallback backends.

**Independent Test**: Configure a mock 503 primary and working fallback; request succeeds via fallback.

### Tests for User Story 2

- [X] T019 [P] [US2] Unit test: PROXY_CONFIG YAML deserialization with valid config, empty config, malformed YAML in crates/proxy/src/fallback/config.rs
- [X] T020 [P] [US2] Unit test: should_fallback() returns true for 500/502/503/429/connection-error, false for 400/401/404 in crates/proxy/src/fallback/mod.rs
- [X] T021 [US2] Integration test: mock 503 primary falls back to secondary; mock 400 primary does NOT fallback; all fail returns last error + x-anyllm-fallback-exhausted header in crates/proxy/tests/fallback.rs

### Implementation for User Story 2

- [X] T022 [P] [US2] Implement FallbackChainConfig and BackendSpec YAML deserialization (PROXY_CONFIG env var path) in crates/proxy/src/fallback/config.rs
- [X] T023 [US2] Implement FallbackChain type with should_fallback() predicate (5xx/429/connection/timeout = true, 4xx except 429 = false) and attempt_with_fallback() loop in crates/proxy/src/fallback/mod.rs
- [X] T024 [US2] Wire fallback into backend dispatch: wrap primary call with fallback chain iteration; log each attempt (backend name + failure reason) per FR-014 in crates/proxy/src/backend/mod.rs
- [X] T025 [US2] Set x-anyllm-fallback-exhausted: true header when all backends fail; on mid-stream failure, terminate SSE with error event per FR-015

**Checkpoint**: US2 functional. Backend failures transparently fall over.

---

## Phase 5: User Story 3 — Async Batch Job Submission (Priority: P1)

**Goal**: Upload JSONL, create batch, poll status, retrieve output. Delegates to backend batch API.

**Independent Test**: Upload 3-line JSONL, create batch, poll until completed, verify 3 output records.

### Tests for User Story 3

- [X] T026 [P] [US3] Unit test: JSONL validation (valid lines, missing custom_id, missing body.model, duplicate custom_id, oversized file) in crates/proxy/src/batch/mod.rs
- [X] T027 [P] [US3] Unit test: batch SQLite CRUD (insert file, insert job, update status, list with pagination) in crates/proxy/src/batch/db.rs
- [X] T028 [US3] Integration test: upload file + create batch + poll status + list batches + verify 501 on unsupported backend in crates/proxy/tests/batch_api.rs

### Implementation for User Story 3

- [X] T029 [P] [US3] Implement BatchFile and BatchJob Rust types, BatchStatus enum, and JSONL validator (custom_id presence/uniqueness/length, body.model, line count cap 50k) in crates/proxy/src/batch/mod.rs
- [X] T030 [P] [US3] Implement batch SQLite CRUD: insert_batch_file, insert_batch_job, get_batch_job, update_batch_job_status, list_batch_jobs in crates/proxy/src/batch/db.rs
- [X] T031 [US3] Implement POST /v1/files handler: accept multipart with purpose=batch, validate JSONL, store in batch_file table, return file object in crates/proxy/src/batch/routes.rs
- [X] T032 [US3] Implement POST /v1/batches handler: validate input_file_id exists, delegate to backend POST /v1/batches, store job record, return 501 for unsupported backends in crates/proxy/src/batch/routes.rs
- [X] T033 [US3] Implement GET /v1/batches/{id} handler: fetch local job, poll backend for status update, persist changes in crates/proxy/src/batch/routes.rs
- [X] T034 [US3] Implement GET /v1/batches list handler with limit + after cursor pagination in crates/proxy/src/batch/routes.rs
- [X] T035 [US3] Replace POST /v1/messages/batches 400 stub with handler per FR-025 in crates/proxy/src/server/routes.rs
- [X] T036 [US3] Register all batch routes (/v1/files, /v1/batches, /v1/batches/{id}) in axum router in crates/proxy/src/server/routes.rs

**Checkpoint**: US3 functional. Batch jobs can be submitted, polled, and listed.

---

## Phase 6: User Story 4 — Per-Request Cost Visibility (Priority: P2)

**Goal**: Every completed request records estimated USD cost; admin API exposes per-key spend.

**Independent Test**: Send request through virtual key, query GET /admin/api/keys/{id}/spend, verify non-zero USD.

### Tests for User Story 4

- [X] T037 [P] [US4] Unit test: cost_for_usage() with exact match, prefix match, and unknown model (returns 0.0 + warn) in crates/proxy/src/cost/mod.rs
- [X] T038 [P] [US4] Unit test: accumulate_spend() increments total_spend, total_input/output_tokens, request_count in SQLite in crates/proxy/src/cost/db.rs
- [X] T039 [US4] Integration test: request through virtual key records spend; spend endpoint returns correct totals; streaming records from usage chunk; unknown model records $0 in crates/proxy/tests/cost_tracking.rs

### Implementation for User Story 4

- [X] T040 [P] [US4] Implement ModelPricing loader (include_str! from assets/model_pricing.json, deserialize to Vec<ModelPricingEntry>, build lookup HashMap) and cost_for_usage(model, input_tokens, output_tokens) with exact then prefix match in crates/proxy/src/cost/mod.rs
- [X] T041 [P] [US4] Implement accumulate_spend() (increment total_spend, total_input_tokens, total_output_tokens, total_requests for virtual key) and get_key_spend() in crates/proxy/src/cost/db.rs
- [X] T042 [US4] Wire cost recording after completed non-streaming requests: compute cost, call accumulate_spend, set x-anyllm-cost-usd header in crates/proxy/src/server/routes.rs
- [X] T043 [US4] Wire cost recording from terminal SSE usage chunk for streaming requests in crates/proxy/src/server/streaming.rs and crates/proxy/src/server/chat_completions.rs
- [X] T044 [US4] Implement GET /admin/api/keys/{id}/spend endpoint returning {key_id, key_prefix, total_cost_usd, total_input_tokens, total_output_tokens, request_count, period_cost_usd, period_start, budget_duration, max_budget_usd} in crates/proxy/src/admin/spend.rs
- [X] T045 [US4] Register /admin/api/keys/{id}/spend route in admin router in crates/proxy/src/admin/routes.rs

**Checkpoint**: US4 functional. Per-key spend visible via admin API and response headers.

---

## Phase 7: User Story 5 — Per-Key Spend Budget Enforcement (Priority: P2)

**Goal**: Keys with max_budget_usd reject requests with 429 budget_exceeded when spend limit is reached.

**Depends on**: US4 (cost tracking must be working for spend to accumulate)

**Independent Test**: Create key with max_budget_usd=0.0001, send request, next request returns 429.

### Tests for User Story 5

- [X] T046 [US5] Integration test: key with $0.0001 budget blocks after first request; 429 body has type=budget_exceeded; budget resets on new period; no-duration budget stays blocked until admin resets in crates/proxy/tests/virtual_keys.rs

### Implementation for User Story 5

- [X] T047 [US5] Implement budget_check() in auth middleware: if max_budget_usd set and period_spend >= limit, return 429 with budget_exceeded body distinguishable from rate_limit_exceeded in crates/proxy/src/server/middleware.rs
- [X] T048 [US5] Implement lazy period reset: on request, check if current time > period boundary (daily=UTC midnight, monthly=1st of month); if so, reset period_spend_usd=0 in DashMap and SQLite in crates/proxy/src/admin/keys.rs
- [X] T049 [US5] Wire budget fields (max_budget_usd, budget_duration) into POST /admin/api/keys create endpoint request/response in crates/proxy/src/admin/routes.rs

**Checkpoint**: US5 functional. Budget-limited keys reject excess spend.

---

## Phase 8: User Story 6 — Role-Based Key Permissions (Priority: P3)

**Goal**: Developer keys access LLM endpoints only; admin keys access everything.

**Independent Test**: Developer key succeeds on /v1/messages, gets 403 on /admin/api/keys.

### Tests for User Story 6

- [X] T050 [US6] Integration test: developer key succeeds on /v1/messages and 403 on /admin/; admin key succeeds on both; new keys default to developer role; static env-var keys act as admin in crates/proxy/tests/virtual_keys.rs

### Implementation for User Story 6

- [X] T051 [US6] Implement role-based path check in auth middleware: if key role=developer and path starts with /admin/, return 403 with permission_denied body; static env-var keys treated as admin in crates/proxy/src/server/middleware.rs
- [X] T052 [US6] Wire role field into POST /admin/api/keys create request and GET /admin/api/keys list response in crates/proxy/src/admin/routes.rs

**Checkpoint**: US6 functional. RBAC enforced on admin vs. LLM endpoints.

---

## Phase 9: User Story 7 — Audio Transcription and Text-to-Speech Passthrough (Priority: P3)

**Goal**: Proxy forwards audio requests to backend unchanged, returns response unchanged.

**Independent Test**: Send POST /v1/audio/transcriptions with audio file, verify response matches backend.

### Tests for User Story 7

- [X] T053 [US7] Integration test: audio transcription passthrough returns backend response; audio speech streams bytes; 501 for anthropic/bedrock backends in crates/proxy/tests/audio_image.rs

### Implementation for User Story 7

- [X] T054 [P] [US7] Implement POST /v1/audio/transcriptions handler: forward multipart/form-data to backend, return response unchanged in crates/proxy/src/server/audio.rs
- [X] T055 [P] [US7] Implement POST /v1/audio/speech handler: forward JSON body to backend, stream audio response bytes in crates/proxy/src/server/audio.rs
- [X] T056 [US7] Register audio routes conditionally (skip for BACKEND=anthropic and BACKEND=bedrock) in crates/proxy/src/server/routes.rs

**Checkpoint**: US7 functional. Audio endpoints working as passthroughs.

---

## Phase 10: User Story 8 — Image Generation Passthrough (Priority: P3)

**Goal**: Proxy forwards image generation requests to backend unchanged.

**Independent Test**: Send POST /v1/images/generations, verify response matches backend.

### Tests for User Story 8

- [X] T057 [US8] Integration test: image generation passthrough returns backend response; 501 for anthropic/bedrock backends in crates/proxy/tests/audio_image.rs

### Implementation for User Story 8

- [X] T058 [P] [US8] Implement POST /v1/images/generations passthrough handler in crates/proxy/src/server/images.rs
- [X] T059 [US8] Register image route conditionally (skip for BACKEND=anthropic and BACKEND=bedrock) in crates/proxy/src/server/routes.rs

**Checkpoint**: US8 functional. Image generation endpoint working.

---

## Phase 11: User Story 9 — Semantic Caching for Similar Prompts (Priority: P4)

**Goal**: Semantically similar prompts return cached responses with x-anyllm-cache: semantic-hit.

**Depends on**: US1 (exact-match cache infrastructure must exist)

**Independent Test**: Send two semantically similar but textually different prompts; second returns semantic-hit.

### Tests for User Story 9

- [X] T060 [US9] Integration test: semantically similar prompt returns semantic-hit; below-threshold prompt returns miss; no QDRANT_URL falls back to exact-match with warning in crates/proxy/tests/cache.rs

### Implementation for User Story 9

- [X] T061 [US9] Implement SemanticCache (embed prompt via /v1/embeddings, search Qdrant collection, return if similarity >= threshold) behind --features qdrant in crates/proxy/src/cache/semantic.rs
- [X] T062 [US9] Wire semantic cache lookup before exact-match in cache middleware; set x-anyllm-cache: semantic-hit on match
- [X] T063 [US9] Graceful startup: if --features qdrant enabled but QDRANT_URL unset, log warning and disable semantic caching (fall back to exact-match only)

**Checkpoint**: US9 functional. Semantic caching available with Qdrant.

---

## Phase 12: Polish & Cross-Cutting Concerns

**Purpose**: Quality gates and documentation

- [X] T064 Verify `cargo clippy -- -D warnings` passes clean on all new code
- [X] T065 Verify `cargo fmt --check` passes clean
- [X] T066 Verify all existing ~555 tests still pass alongside new tests
- [X] T067 Verify all new source files (excl. test modules) are under 400 lines per constitution III
- [X] T068 Update docs/COMPARISON_LITELLM.md to reflect closed gaps (caching, fallback, batch, cost, budget, RBAC, audio, image, semantic cache)
- [X] T069 Run quickstart.md scenarios end-to-end as a manual smoke test

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies, start immediately
- **Foundational (Phase 2)**: Depends on Setup
- **User Stories (Phase 3-11)**: All depend on Foundational; Tier 1 (US1-US4) should complete before Tier 2 (US5-US9) per spec assumption
- **US5 (Budget)**: Depends on US4 (Cost Tracking) being complete
- **US9 (Semantic Cache)**: Depends on US1 (Caching) being complete
- **Polish (Phase 12)**: Depends on all desired user stories

### User Story Dependencies

```
Foundational
  ├── US1 (Caching)  ─────────────────┬──→ US9 (Semantic Cache)
  ├── US2 (Fallback) ──────────────┐  │
  ├── US3 (Batch)    ──────────┐   │  │
  │                            │   │  │
  ├── US4 (Cost Tracking) ────→ US5 (Budget)
  │                            │   │  │
  ├── US6 (RBAC)     ─────┐   │   │  │
  ├── US7 (Audio)    ──┐  │   │   │  │
  └── US8 (Image)   ─┐ │  │   │   │  │
                      ↓ ↓  ↓   ↓   ↓  ↓
                      Polish (Phase 12)
```

- US1, US2, US3 are independent of each other (all P1, Tier 1)
- US4 is independent but gates US5
- US6, US7, US8 are independent of each other (all Tier 2)
- US9 depends on US1

### Within Each User Story

1. Unit tests can run in parallel [P]
2. Implementation tasks follow: types → CRUD → handlers → wiring → integration test
3. Integration test validates the story before moving on

### Parallel Opportunities

**After Foundational completes:**
- US1 + US2 + US3 can all start in parallel (independent Tier 1 stories)
- US6 + US7 + US8 can all start in parallel (independent Tier 2 stories)
- Within US7: T054 and T055 can run in parallel (different handler functions)
- Within US3: T029 and T030 can run in parallel (types vs DB)

---

## Parallel Example: Tier 1 Stories

```
Agent A: US1 (Caching) — T010..T018
Agent B: US2 (Fallback) — T019..T025
Agent C: US3 (Batch) — T026..T036
```

## Parallel Example: Tier 2 Independent Stories

```
Agent A: US6 (RBAC) — T050..T052
Agent B: US7 (Audio) — T053..T056
Agent C: US8 (Image) — T057..T059
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1 (Caching)
4. **STOP and VALIDATE**: cargo test passes, cache hit/miss works
5. Deploy/demo if ready

### Tier 1 Delivery (US1 + US2 + US3 + US4)

1. Setup + Foundational
2. US1 (Caching) + US2 (Fallback) + US3 (Batch) — parallel or sequential
3. US4 (Cost Tracking) — can overlap with US1-US3
4. **VALIDATE**: all Tier 1 stories pass independently, existing tests still pass
5. Merge Tier 1

### Full Delivery

1. Tier 1 complete and validated
2. US5 (Budget) — requires US4
3. US6 (RBAC) + US7 (Audio) + US8 (Image) — parallel
4. US9 (Semantic Cache) — requires US1, lowest priority
5. Polish phase
6. **VALIDATE**: all tests pass, clippy clean, fmt clean, files under 400 lines

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps task to specific user story for traceability
- Each user story is independently completable and testable after Foundational phase
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Constitution §II: test every non-trivial behavior before considering work complete
- Constitution §III: all new source files (excl. tests) must stay under 400 lines
