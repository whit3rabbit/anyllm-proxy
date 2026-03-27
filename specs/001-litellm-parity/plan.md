# Implementation Plan: LiteLLM Parity — Remaining Gap Closure

**Branch**: `001-litellm-parity` | **Date**: 2026-03-26 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/001-litellm-parity/spec.md`

## Summary

Close the eight LiteLLM parity gaps identified in `docs/COMPARISON_LITELLM.md`. Tier 1 (caching,
fallback chains, batch processing, cost tracking) delivers production-gateway value directly. Tier 2
(budget enforcement, RBAC, audio/image passthroughs, semantic caching) adds enterprise and
multi-modal surface. All new code lands in the `crates/proxy` crate; the translator crate stays
IO-free. Implementation is sequenced Tier 1 before Tier 2; all existing ~555 tests must pass after
each tier.

## Technical Context

**Language/Version**: Rust stable (1.83+, workspace edition 2021)
**Primary Dependencies (existing)**: axum 0.8, reqwest 0.12, tokio 1, serde/serde_json 1,
  rusqlite 0.32 (bundled), dashmap 6, sha2 0.10, uuid 1, tracing 0.1, toml 1, tiktoken-rs 0.9
**New Production Dependencies**:
  - `moka` 0.12 — async TTL/LRU cache (in-memory Tier 1 cache)
  - `serde_yaml` 0.9 — PROXY_CONFIG YAML deserialization (fallback chain config)
**New Optional Feature Dependencies**:
  - `redis` 0.27 with `tokio-comp` — Redis cache tier (`--features redis`)
  - `qdrant-client` 1.x — semantic cache vector store (`--features qdrant`)
**Storage**: SQLite (existing, extended with new tables); Redis (optional Tier 1 cache); Qdrant
  (optional semantic cache)
**Testing**: `cargo test` (unit + integration); fixture JSON files in `fixtures/`
**Target Platform**: Linux server (single-process proxy)
**Project Type**: web-service (axum-based HTTP proxy)
**Performance Goals**: Cache hits must not add >5ms latency vs a cache miss on the hot path.
  No specific throughput target stated for other features.
**Constraints**: All source files (excl. tests) <400 lines (constitution §III). No unwrap/expect in
  production paths. New deps require justification. `cargo clippy -- -D warnings` must pass.
**Scale/Scope**: Single-proxy instance; virtual key count expected <10k; batch jobs <100k
  concurrent (delegated to backend).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Gate | Status | Notes |
|------|--------|-------|
| Security — auth/access-control changes | REQUIRES REVIEW | RBAC changes auth middleware; budget enforcement adds a rejection path. Both require documented review before merge. |
| Security — untrusted input validated at boundary | PASS (ongoing) | New endpoints (audio, images, batch files) must size-bound inputs per existing 32MB middleware; JSONL upload needs its own cap (FR-025 implies file size limit). |
| Security — no secrets in source/logs | PASS | No new secrets introduced; cost tracking records USD amounts only. |
| Dependency policy | REQUIRES JUSTIFICATION | 2 new prod deps (`moka`, `serde_yaml`); 2 new optional deps (`redis`, `qdrant-client`). Justifications in research.md §Dependencies. |
| Test coverage — new logic tested before complete | PASS (enforced) | Each tier's tasks include unit and integration tests before the tier is closed. |
| File size discipline | PRE-EXISTING VIOLATIONS | `admin/routes.rs` 853 lines, `admin/db.rs` 629 lines, `server/routes.rs` 679 lines already violate 400-line rule. New features MUST go in new modules; do not extend these files. Tracked in Complexity Tracking below. |
| Code quality — clippy/fmt clean | PASS (ongoing) | Must be verified per commit. |
| Minimal changes — no drive-by refactors | PASS (ongoing) | Pre-existing violations are not in scope for this feature. |

**Post-Phase-1 re-check**: After data-model.md and contracts/ are complete, re-verify that no
proposed schema or API shape requires a change to a file already at/over the 400-line limit without
a corresponding split.

## Project Structure

### Documentation (this feature)

```text
specs/001-litellm-parity/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   ├── cache-headers.md
│   ├── batch-api.md
│   ├── cost-tracking-api.md
│   ├── budget-rbac-api.md
│   └── audio-image-api.md
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/proxy/src/
├── cache/
│   ├── mod.rs           # CacheBackend trait, CacheEntry type, key hashing (SHA-256)
│   ├── memory.rs        # moka-backed in-memory cache implementation
│   └── redis.rs         # Redis-backed cache tier (feature = "redis")
├── fallback/
│   ├── mod.rs           # FallbackChain type, should_fallback predicate, attempt loop
│   └── config.rs        # PROXY_CONFIG YAML deserialization, BackendSpec
├── batch/
│   ├── mod.rs           # BatchJob, BatchFile types, status machine
│   ├── routes.rs        # axum handlers: POST /v1/files, POST /v1/batches, GET /v1/batches/{id}, GET /v1/batches
│   └── db.rs            # SQLite CRUD for batch_file and batch_job tables
├── cost/
│   ├── mod.rs           # ModelPricing table (include_str! embedded JSON), cost_for_usage()
│   └── db.rs            # SQLite spend accumulation and GET /admin/api/keys/{id}/spend
├── server/
│   ├── audio.rs         # POST /v1/audio/transcriptions, POST /v1/audio/speech passthrough
│   └── images.rs        # POST /v1/images/generations passthrough
└── (existing modules unchanged except for targeted extension points)

crates/proxy/src/admin/
└── spend.rs             # Admin routes for spend queries (split out of routes.rs)

assets/
└── model_pricing.json   # Bundled model pricing table (embedded via include_str!)
```

**Structure Decision**: Single-project Rust workspace. All new code in `crates/proxy`. The translator
crate is unchanged (IO-free constraint). New top-level modules (`cache/`, `fallback/`, `batch/`,
`cost/`) keep feature concerns isolated and all new source files well under 400 lines.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| `admin/routes.rs` 853 lines (pre-existing) | Accumulated during prior milestones; not in scope to fix here | Splitting it is a refactor outside this feature's scope; doing so risks regressions and scope creep. New admin spend routes go in `admin/spend.rs` instead. |
| `admin/db.rs` 629 lines (pre-existing) | Same — accumulated during prior milestones | New batch and cost DB functions go in `batch/db.rs` and `cost/db.rs` respectively; no lines added to `admin/db.rs`. |
| `server/routes.rs` 679 lines (pre-existing) | Same | New audio/image/batch routes registered via helper functions in their own modules; `server/routes.rs` gains only route registrations (minimal additions). |
