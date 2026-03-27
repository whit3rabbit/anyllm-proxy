# Research: LiteLLM Parity — Remaining Gap Closure

**Phase 0 output** | Branch: `001-litellm-parity` | Date: 2026-03-26

All NEEDS CLARIFICATION items from the Technical Context are resolved below.

---

## Dependencies

### In-Memory Cache Crate

**Decision**: `moka` v0.12 (`moka = { version = "0.12", features = ["future"] }`)

**Rationale**: moka is the de-facto standard async TTL cache in the Rust ecosystem. It supports
per-entry TTL, maximum capacity bounds, async insert/get via `Cache::get_with`, and is `Arc`-safe.
The `future` feature enables async eviction listeners. Actively maintained; used in production by
large Rust services.

**Alternatives considered**:
- `mini-moka` — lighter but lacks async support; insufficient for tokio context.
- `dashmap` with manual TTL — already in project; would require bespoke expiry sweep task,
  maintenance burden, and no size bounds. Rejected.
- `lru` crate — no TTL support. Rejected.

---

### Redis Client

**Decision**: `redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }`
under `[features] redis = ["dep:redis"]`

**Rationale**: `redis-rs` (the `redis` crate) is the canonical async Redis client for Rust.
The `tokio-comp` feature enables `tokio`-backed async IO; `connection-manager` provides
automatic reconnection without a pooling library. This keeps the optional dependency minimal.
`deadpool-redis` adds pooling but is unnecessary for a proxy that serializes Redis lookups
through a single async task.

**Alternatives considered**:
- `deadpool-redis` — adds connection pooling; overkill for current access patterns. Rejected.
- `fred` — newer, feature-rich; introduces more code surface for a feature that is optional. Rejected.

---

### Semantic Cache Vector Store

**Decision**: `qdrant-client = { version = "1", optional = true }` under `[features] qdrant = ["dep:qdrant-client"]`

**Rationale**: qdrant-client is the official Rust client for Qdrant. Async-native, well-documented.
Semantic caching (P4) is deferred until Tier 1+2 pass; this crate is only pulled in with
`--features qdrant`.

**Alternatives considered**:
- Direct HTTP calls to Qdrant REST API — avoids the dep but requires bespoke (de)serialization
  for vectors, payloads, and search results. Rejected in favor of the typed client.

---

### PROXY_CONFIG Format

**Decision**: YAML via `serde_yaml = "0.9"` as a new production dependency.

**Rationale**: FR-010 explicitly specifies YAML for `PROXY_CONFIG`. `serde_yaml` is the standard
serde adapter for YAML in Rust; `serde_yml` (maintained fork) is also viable but `serde_yaml` has
wider adoption. The config is small (fallback backend lists per route) so the parse overhead is
negligible at startup.

**Alternatives considered**:
- TOML (`toml` already in project) — would satisfy the schema but contradicts the spec's explicit
  "YAML" requirement. Rejected to stay spec-compliant.
- JSON — already present; same issue, contradicts spec. Rejected.
- `figment` — multi-source config library; more machinery than needed for a single YAML file. Rejected.

---

### Model Pricing Table

**Decision**: A static JSON file at `assets/model_pricing.json`, embedded at compile time via
`include_str!("../../assets/model_pricing.json")` and deserialized once at startup into a
`HashMap<String, PricingEntry>`.

**Rationale**: The spec says "ships as a static JSON file bundled with the binary; live-fetch is
out of scope." `include_str!` is idiomatic Rust for compile-time embedding; no new dep required.
Pricing is keyed by model name pattern (exact match first, then prefix match). The table covers at
minimum: GPT-4o/4/3.5 variants, Claude 3/3.5/3.7 variants, Gemini 1.5/2.0/2.5 variants, and
common embedding models.

**Source for initial prices**: Published at time of plan (2026-03-26):
- OpenAI: https://openai.com/api/pricing (GPT-4o: $2.50/$10.00 per 1M tokens input/output)
- Anthropic: https://www.anthropic.com/pricing (Claude Sonnet 4.6: $3/$15 per 1M)
- Google: https://ai.google.dev/pricing (Gemini 2.5 Pro: $1.25/$10 per 1M)

The file will be maintained manually; no auto-update mechanism.

---

## Batch Processing Architecture

**Decision**: The proxy delegates to the configured backend's native batch API (OpenAI and Azure).
The proxy stores the uploaded JSONL file in SQLite (as a BLOB for small files, path reference for
large), creates a batch job record, calls `POST /v1/batches` on the backend, and stores the
backend-assigned batch ID. Polling and output retrieval forward to the backend's batch endpoints.

**Why delegation**: The spec states "batch processing MUST be supported for OpenAI and Azure
backends; delegates actual inference to the backend." Building an internal job queue is explicitly
out of scope. The proxy acts as a thin routing and auth layer for batch operations.

**Flow**:
1. `POST /v1/files` → store JSONL in SQLite `batch_file` table, return file ID
2. `POST /v1/batches` → validate file exists, call backend `POST /v1/batches`, store job in
   `batch_job` table with backend job ID, return batch object
3. `GET /v1/batches/{id}` → fetch from `batch_job`, forward status poll to backend, update local
   status, return current status
4. `GET /v1/batches/{id}/output_file` → fetch output from backend, stream to client

**JSONL line limit**: Reject files with content-length > 100MB (per OpenAI's own limit); returns
413. Validated at upload time.

**Unsupported backends**: Vertex, Gemini, Anthropic passthrough, Bedrock → return 501 with body
`{"error": {"type": "not_supported", "message": "Batch processing is not available for this backend."}}`.

---

## Fallback Chain Design

**Decision**: Fallback chains are configured per-route in a `PROXY_CONFIG` YAML file. The path is
set via the `PROXY_CONFIG` environment variable. If unset, fallback is disabled.

**Config shape** (YAML):
```yaml
fallback_chains:
  default:
    - name: azure
      env_prefix: AZURE_FALLBACK_
    - name: openai
      env_prefix: OPENAI_FALLBACK_
```

Each entry in the chain is a named backend with env-var prefix for its credentials. The proxy
builds a `FallbackChain` from this config at startup; if `PROXY_CONFIG` is unset, the chain is
empty (no fallback).

**Trigger conditions** (FR-011, FR-012, FR-013): 5xx, 429, or connection/timeout errors trigger
fallback. 4xx other than 429 do NOT trigger fallback.

**Mid-stream**: If SSE has started (first chunk sent), fallback is not attempted. The stream is
closed with an error event. This is explicitly out of scope per FR-015.

---

## Cache Key Design

**Decision**: SHA-256 hash of a canonical JSON representation of `{model, messages, temperature,
top_p, max_tokens, stop, tools, tool_choice}` fields, prefixed by endpoint namespace
(`anth:` for Anthropic format, `oai:` for OpenAI format).

**Canonical JSON**: fields sorted alphabetically, no extra whitespace, missing optional fields
omitted (not included as null). `serde_json::to_string` with sorted keys via `BTreeMap` or
custom serializer.

**Cache namespace**: FR-006 requires Anthropic and OpenAI format requests not share entries even
with identical content. The `anth:` / `oai:` prefix ensures this.

**Why SHA-256**: already in project (`sha2` crate); 32-byte digest fits in a SQLite TEXT column
as hex. Collision probability negligible.

---

## RBAC Implementation

**Decision**: Add a `role` column to the `virtual_api_key` SQLite table. Roles: `admin` and
`developer`. Static env-var keys (from `PROXY_API_KEYS`) are treated as `admin` role at runtime
without a DB lookup.

The auth middleware already identifies whether a key is a virtual key or static key. The RBAC
check is a single comparison added to the middleware:
- If the key is a static key → admin role → all paths permitted.
- If the key is a virtual key and its role is `developer` → reject any path starting with `/admin/`
  with 403.
- If the key is a virtual key and its role is `admin` → all paths permitted.

No new middleware layer; the check is inlined in the existing `validate_auth` middleware
(or a thin helper called from it).

---

## Budget Enforcement Approach

**Decision**: Budget enforcement runs as a pre-flight check in the auth middleware, after virtual
key lookup. The check reads `max_budget_usd` and `current_period_spend_usd` from the DashMap
cache (populated at startup and updated after each request). If `current_period_spend_usd >=
max_budget_usd`, return 429 with `{"error": {"type": "budget_exceeded", ...}}`.

**Period reset**: Checked lazily at request time. If `budget_duration` is set and the current
time exceeds the period boundary (daily: UTC midnight; monthly: first day of month UTC midnight),
reset `current_period_spend_usd` to 0 in both DashMap and SQLite before checking.

**In-flight race**: Acceptable per spec assumption §Budget period resets: "the in-flight request
completes; the next request is the first to be rejected." The spend update (post-response) is not
transactionally guarded against the pre-flight check; slight overspend by at most one request is
acceptable.

---

## Audio / Image Passthrough

**Decision**: Both are transparent passthroughs using `reqwest` to forward the raw request body
and headers to the backend's corresponding endpoint. No translation, no model mapping. Response
body is streamed back byte-for-byte.

**Multipart forms**: Audio transcription uses `multipart/form-data`. axum's `Multipart` extractor
reads the body; the proxy forwards the raw bytes via `reqwest::multipart::Form`. Image generation
uses JSON; direct passthrough.

**Backend restriction**: Not mounted for `Anthropic` passthrough or `Bedrock` backends (FR-063,
FR-071). The router conditionally registers these routes based on the configured backend.

---

## Open Items (none)

All NEEDS CLARIFICATION items are resolved. No open items remain before Phase 1.
