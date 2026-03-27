# Feature Specification: LiteLLM Parity — Remaining Gap Closure

**Feature Branch**: `001-litellm-parity`
**Created**: 2026-03-26
**Status**: Draft
**Input**: Implement all remaining LiteLLM parity features: response caching, cross-provider fallback chains, batch processing, cost tracking, RBAC, budget enforcement, audio/image/reranking endpoints, and semantic caching.

---

## Context

anyllm-proxy is a specialized protocol translator (Anthropic API in, OpenAI-compatible backend out).
LiteLLM is a broad AI gateway. The gap comparison in `docs/COMPARISON_LITELLM.md` identifies eight
remaining areas where anyllm-proxy lacks features that LiteLLM users rely on. This spec drives
closure of those gaps in priority order, enabling anyllm-proxy to serve as a production AI gateway,
not just a translation shim.

Features are grouped into two tiers matching the comparison doc:
- **Tier 1** (high value, broad applicability): caching, fallback chains, batch processing, cost tracking
- **Tier 2** (enterprise/niche): RBAC, budget enforcement, audio/image/reranking, semantic caching

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Repeated Prompts Return From Cache (Priority: P1)

A developer runs an evaluation harness that sends the same 50 prompts to the proxy hundreds of times
during a test run. Today every request hits the backend and costs money. With caching, identical
requests return the cached response in milliseconds at zero upstream cost.

**Why this priority**: Directly reduces cost and latency for the most common repetition patterns
(evals, demos, retries on network flap). Delivers value without any client-side changes.

**Independent Test**: Send `POST /v1/messages` with the same body twice. The second response must
arrive faster and include `x-anyllm-cache: hit`. Upstream call count must be 1, not 2.

**Acceptance Scenarios**:

1. **Given** an in-memory cache is configured, **When** the same request body is sent twice within
   the TTL window, **Then** the second response is returned from cache with no upstream call made
   and a `x-anyllm-cache: hit` response header.
2. **Given** a TTL of 60 seconds, **When** the cache entry expires and the same request is sent,
   **Then** the proxy makes a fresh upstream call and caches the new response.
3. **Given** two requests with identical bodies except `temperature` differs, **When** both are
   sent, **Then** both are treated as cache misses and both upstream calls are made.
4. **Given** a Redis cache backend is configured, **When** the proxy restarts and the same request
   is sent, **Then** the response is returned from Redis without an upstream call.

---

### User Story 2 — Transparent Backend Failover (Priority: P1)

A team runs anyllm-proxy against their primary OpenAI deployment. When OpenAI returns 5xx errors or
rate limits, requests fail to the client. With fallback chains, the proxy silently retries against
a configured alternate backend and the client receives a valid response.

**Why this priority**: Improves reliability without client changes. Essential for production
deployments where uptime matters more than which model answered.

**Independent Test**: Configure a primary backend that returns 503. Configure a fallback backend
that works. Send a request. Verify a valid response is returned and proxy logs show fallback used.

**Acceptance Scenarios**:

1. **Given** a primary backend returns 503, **When** a request is sent, **Then** the proxy retries
   against the configured fallback backend and returns a successful response to the client.
2. **Given** a primary backend returns 429, **When** a request is sent, **Then** the proxy
   attempts the fallback backend instead of passing the 429 to the client.
3. **Given** all configured backends fail, **When** a request is sent, **Then** the proxy returns
   an error to the client after exhausting all fallback options.
4. **Given** streaming is active on the primary backend and it fails mid-stream, **Then** the
   proxy closes the stream and returns an error (mid-stream backend switching is out of scope).

---

### User Story 3 — Async Batch Job Submission (Priority: P1)

A data team wants to process 10,000 prompts overnight without holding open connections. They submit
a JSONL file of requests, get a batch ID back immediately, poll for completion, then retrieve
results. Today the proxy stubs `POST /v1/messages/batches` with a 400.

**Why this priority**: Unlocks overnight processing workloads that cannot use synchronous calls.
Also closes the most visible stubbed endpoint in the comparison doc.

**Independent Test**: Upload a JSONL file with 3 requests. Create a batch. Poll status until
`completed`. Retrieve output. Verify 3 response objects, each with a matching `custom_id`.

**Acceptance Scenarios**:

1. **Given** a valid JSONL file is uploaded via `POST /v1/files`, **When** a batch is created
   referencing that file, **Then** the proxy returns a batch object with a unique batch ID and
   status `validating` or `in_progress`.
2. **Given** a batch is processing, **When** `GET /v1/batches/{id}` is polled, **Then** the
   response reflects the current status (`in_progress`, `completed`, `failed`).
3. **Given** a batch completes, **When** the output file is retrieved, **Then** the JSONL output
   contains one response object per input request, each with the original `custom_id`.
4. **Given** a batch request line contains an invalid body, **When** the batch completes, **Then**
   that line produces an error object; other lines succeed.

---

### User Story 4 — Per-Request Cost Visibility (Priority: P2)

An operator wants to understand how much each virtual key costs per day. Today the proxy has no
cost data. With cost tracking, every request records an estimated USD cost. The admin API exposes
per-key spend summaries.

**Why this priority**: Required before budget enforcement can be meaningful. Unblocks
showback/chargeback use cases without a third-party observability service.

**Independent Test**: Send a request through a virtual key. Query `GET /admin/api/keys/{id}/spend`.
Verify a non-zero USD amount is returned matching approximate token cost for that model.

**Acceptance Scenarios**:

1. **Given** a request completes through a virtual key, **When** token counts are returned by the
   backend, **Then** the proxy records an estimated USD cost using the bundled pricing table.
2. **Given** a model is not in the pricing table, **When** a request completes, **Then** cost is
   recorded as zero and a warning is emitted in the proxy log.
3. **Given** a virtual key has made multiple requests, **When** `GET /admin/api/keys/{id}/spend`
   is called, **Then** the response includes total tokens and total estimated USD spend.
4. **Given** a streaming request completes, **When** token counts are present in the terminal
   usage event, **Then** cost is recorded using those counts (not zero).

---

### User Story 5 — Per-Key Spend Budget Enforcement (Priority: P2)

An operator sets a maximum monthly spend of $50 on a virtual key. When accumulated spend reaches
that limit, the proxy rejects requests with a 429 budget-exceeded error rather than continuing to
run up costs.

**Why this priority**: Required for multi-tenant deployments where different teams share one proxy.
Without it, a runaway script can exhaust a shared API key budget.

**Independent Test**: Create a key with `max_budget_usd: 0.0001`. Send one request. Verify the
next request returns 429 with a budget-exhausted error body.

**Acceptance Scenarios**:

1. **Given** a virtual key has a `max_budget_usd` set, **When** accumulated spend reaches or
   exceeds that budget, **Then** subsequent requests return 429 with a budget-exhausted error.
2. **Given** no `budget_duration` is set, **When** budget is exhausted, **Then** the key remains
   blocked until an admin raises or resets the budget.
3. **Given** a `budget_duration` of `monthly`, **When** the calendar month resets, **Then**
   accumulated spend resets to zero and requests are accepted again.
4. **Given** a request is in-flight when budget hits the limit, **Then** that request completes;
   the next request is the first to be rejected.

---

### User Story 6 — Role-Based Key Permissions (Priority: P3)

An operator issues API keys to developers that can only make LLM calls, not manage other keys or
view spend. A separate admin key has full access.

**Why this priority**: Required for shared deployments where the proxy API key cannot be given to
end users without also granting admin access.

**Independent Test**: Create a key with `role: developer`. Use it on `POST /v1/messages` (succeeds)
and `POST /admin/api/keys` (403). Use an admin key on the admin call (succeeds).

**Acceptance Scenarios**:

1. **Given** a key with `role: developer`, **When** it is used to make an LLM call, **Then** the
   request proceeds normally.
2. **Given** a key with `role: developer`, **When** it is used to call any `/admin/` endpoint,
   **Then** the proxy returns 403 Forbidden.
3. **Given** a key with `role: admin`, **When** it is used to create another key, **Then** the new
   key is created and returned.
4. **Given** no role is set on a key, **When** the key is used, **Then** it defaults to
   `developer` role behavior.

---

### User Story 7 — Audio Transcription and Text-to-Speech Passthrough (Priority: P3)

A developer integrates anyllm-proxy as a unified LLM gateway and wants to route Whisper
transcription and TTS requests through the same proxy endpoint.

**Why this priority**: Completes the OpenAI-compatible endpoint surface. Low implementation
complexity (passthrough only, no translation).

**Independent Test**: Send `POST /v1/audio/transcriptions` with an audio file. Verify the response
matches the backend's response (passthrough, byte-for-byte comparison).

**Acceptance Scenarios**:

1. **Given** a valid audio file is sent to `POST /v1/audio/transcriptions`, **When** the backend
   returns a transcription, **Then** the proxy returns it unchanged.
2. **Given** a text string is sent to `POST /v1/audio/speech`, **When** the backend returns audio
   bytes, **Then** the proxy streams them to the client unchanged.
3. **Given** the backend returns an error, **Then** the proxy passes through the error response.

---

### User Story 8 — Image Generation Passthrough (Priority: P3)

A developer routes DALL-E image generation requests through the proxy alongside text model calls,
using the same API key and base URL.

**Why this priority**: Passthrough is low complexity; enables the proxy to serve as a single
endpoint gateway for all modalities.

**Independent Test**: Send `POST /v1/images/generations` with a valid prompt. Verify the proxy
returns the backend's image URL(s) unchanged.

**Acceptance Scenarios**:

1. **Given** a valid image generation request, **When** the backend returns image URLs, **Then**
   the proxy returns the response unchanged.
2. **Given** the backend does not support image generation, **When** a request is sent, **Then**
   the backend's error is passed through unchanged.

---

### User Story 9 — Semantic Caching for Similar Prompts (Priority: P4)

A team finds users ask semantically identical questions with slightly different wording. Exact-match
caching misses these. Semantic caching returns cached answers for queries with high embedding
similarity.

**Why this priority**: Requires an embedding backend and adds operational complexity. Valuable only
after basic caching is working.

**Independent Test**: Send two semantically similar but textually different prompts. Verify the
second returns a cache hit with `x-anyllm-cache: semantic-hit` when similarity exceeds the
configured threshold.

**Acceptance Scenarios**:

1. **Given** semantic caching is enabled with a similarity threshold, **When** a semantically
   similar prompt is sent after a cached one, **Then** the cached response is returned with
   `x-anyllm-cache: semantic-hit`.
2. **Given** a prompt's similarity score is below the threshold, **When** it is sent, **Then** a
   fresh upstream call is made and cached.
3. **Given** no embedding backend is configured, **When** semantic caching is enabled, **Then**
   the proxy falls back to exact-match caching with a startup warning.

---

### Edge Cases

- What happens when the Redis cache backend is unreachable? Proxy falls back to in-memory cache
  (or no cache) and logs the error; requests are never blocked by cache failures.
- What happens when all fallback backends are exhausted? Return the last upstream error received
  with a `x-anyllm-fallback-exhausted: true` header.
- What happens when a batch JSONL file exceeds the size limit? Return 413 with a descriptive error.
- What happens when cost tracking fails to record (DB error)? Log the failure and still return
  the LLM response; tracking failure must never block the request path.
- What happens when a budget-enforcement check races with an in-flight request? The in-flight
  request completes; the subsequent request is rejected.
- What happens when a model's pricing is missing from the pricing table? Record $0 cost and emit
  a warning; do not fail the request.

---

## Requirements *(mandatory)*

### Functional Requirements

**Caching (Tier 1)**

- **FR-001**: The proxy MUST support an in-memory response cache keyed on a SHA-256 hash of the
  canonicalized request fields: `{model, messages, temperature, top_p, max_tokens, stop, tools,
  tool_choice}`.
- **FR-002**: The proxy MUST support an optional Redis response cache as a second tier behind
  in-memory, configured via environment variable.
- **FR-003**: Cached responses MUST include `x-anyllm-cache: hit`; non-cached responses MUST
  include `x-anyllm-cache: miss`.
- **FR-004**: Cache TTL MUST be configurable globally via env var and overridable per-request via
  a `cache_ttl_secs` field in the request body.
- **FR-005**: Cache entries MUST be invalidated when TTL expires; no manual invalidation API is
  required for v1.
- **FR-006**: Cache keys MUST be namespaced by endpoint format so Anthropic-format and
  OpenAI-format requests with identical content do not share cache entries.

**Fallback Chains (Tier 1)**

- **FR-010**: The proxy MUST support configuring an ordered list of fallback backend names per
  route in `PROXY_CONFIG` YAML.
- **FR-011**: The proxy MUST attempt fallback backends when the primary returns 5xx or a network
  error (connection refused, timeout).
- **FR-012**: The proxy MUST attempt fallback backends when the primary returns 429.
- **FR-013**: The proxy MUST NOT attempt fallback on 4xx client errors other than 429.
- **FR-014**: Each fallback attempt MUST be logged with backend name and failure reason.
- **FR-015**: Mid-stream backend switching is out of scope; if a streaming request's backend fails
  after SSE has started, the stream is terminated with an error event.

**Batch Processing (Tier 1)**

- **FR-020**: The proxy MUST implement `POST /v1/files` to accept JSONL uploads with
  `purpose=batch`, storing files in SQLite (blob or path reference).
- **FR-021**: The proxy MUST implement `POST /v1/batches` to create a batch job from an uploaded
  file, delegating processing to the configured backend's batch API.
- **FR-022**: The proxy MUST implement `GET /v1/batches/{id}` returning current batch status.
- **FR-023**: The proxy MUST implement `GET /v1/batches` listing recent batches for the
  authenticated key.
- **FR-024**: When a batch completes, output MUST be retrievable as a JSONL file where each line
  contains a `custom_id` matching the corresponding input line.
- **FR-025**: The 400 stub on `POST /v1/messages/batches` MUST be replaced with the batch handler.
- **FR-026**: Batch processing MUST be supported for OpenAI and Azure backends. Unsupported
  backends MUST return 501 with a clear error.

**Cost Tracking (Tier 1)**

- **FR-030**: The proxy MUST ship a bundled model pricing JSON covering at minimum: GPT-4o/4/3.5
  variants, Claude 3/3.5/3.7 variants, Gemini 1.5/2.0/2.5 variants, and common embedding models.
- **FR-031**: After every completed request, the proxy MUST compute estimated USD cost from
  input tokens, output tokens, and the pricing table for the resolved backend model.
- **FR-032**: Computed cost MUST be accumulated in the virtual key's spend record in SQLite.
- **FR-033**: `GET /admin/api/keys/{id}/spend` MUST return `{request_count, total_input_tokens,
  total_output_tokens, total_cost_usd}`.
- **FR-034**: If the model is absent from the pricing table, cost MUST be recorded as 0.0 and a
  `warn` log line emitted.
- **FR-035**: Streaming responses MUST record cost from the terminal `usage` SSE chunk when
  present; if absent, cost is recorded as 0.0 for that request.

**Budget Enforcement (Tier 2)**

- **FR-040**: Virtual keys MUST support an optional `max_budget_usd: f64` field; when set,
  requests MUST be rejected with 429 once accumulated spend meets or exceeds the limit.
- **FR-041**: Virtual keys MUST support an optional `budget_duration: "daily" | "monthly"` field;
  when set, spend MUST reset to zero at the start of each period.
- **FR-042**: Budget checks MUST run in the auth middleware before forwarding to the backend.
- **FR-043**: The 429 for budget exhaustion MUST use error type `budget_exceeded` in the body,
  distinguishable from rate-limit 429s which use `rate_limit_exceeded`.

**RBAC (Tier 2)**

- **FR-050**: Virtual keys MUST support a `role` field with values `admin` (default for static
  env-var keys) and `developer` (default for dynamically issued keys).
- **FR-051**: `developer` keys MUST be permitted on LLM endpoints (`/v1/messages`,
  `/v1/chat/completions`, `/v1/embeddings`, `/v1/audio/*`, `/v1/images/*`) and MUST be rejected
  with 403 on all `/admin/` paths.
- **FR-052**: `admin` keys MUST be permitted on all endpoints.
- **FR-053**: Static API keys set via `PROXY_API_KEYS` env var MUST behave as `admin` role.
- **FR-054**: OIDC/JWT and IP allowlisting are explicitly out of scope.

**Audio Endpoints (Tier 2)**

- **FR-060**: `POST /v1/audio/transcriptions` MUST be implemented as a transparent passthrough to
  the backend; no translation is performed.
- **FR-061**: `POST /v1/audio/speech` MUST be implemented as a transparent passthrough.
- **FR-062**: Both endpoints MUST require proxy authentication.
- **FR-063**: Audio endpoints MUST NOT be mounted for Anthropic passthrough or Bedrock backends.

**Image Generation (Tier 2)**

- **FR-070**: `POST /v1/images/generations` MUST be implemented as a transparent passthrough.
- **FR-071**: Image generation MUST NOT be mounted for Anthropic passthrough or Bedrock backends.

**Semantic Caching (Tier 2)**

- **FR-080**: When semantic caching is enabled, the proxy MUST generate an embedding for each
  incoming prompt and query a configured Qdrant collection for similar cached prompts.
- **FR-081**: If a cached result's similarity score meets or exceeds the configured threshold
  (default: 0.95), the cached response MUST be returned with `x-anyllm-cache: semantic-hit`.
- **FR-082**: When no embedding backend is configured, semantic caching MUST disable gracefully
  at startup with a warning log, falling back to exact-match caching.
- **FR-083**: The semantic cache MUST store `(embedding_vector, request_hash, response_body)` in
  Qdrant with the same TTL logic as exact-match cache.

### Key Entities *(include if feature involves data)*

- **CacheEntry**: key hash, model, response body bytes, creation timestamp, TTL seconds
- **BatchJob**: ID, status (validating/in_progress/completed/failed/expired), input file ID,
  output file ID, backend name, request count, completed count, error count, created timestamp
- **BatchFile**: ID, purpose (batch), byte size, created timestamp, raw content (JSONL)
- **ModelPricingEntry**: model name pattern, input cost per token (USD), output cost per token (USD),
  provider hint
- **SpendRecord**: key ID, total cost USD, total input tokens, total output tokens, request count,
  period start timestamp (for budget reset)
- **VirtualKey** (extended): adds role (`admin`/`developer`), max budget USD (optional), budget
  duration (optional), current period spend USD

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A repeated identical request returns a cached response with no upstream call;
  verified by proxy request log showing exactly one upstream call for two identical sends.
- **SC-002**: When the primary backend is unavailable, all requests reach a configured fallback
  and return valid responses; verified in integration tests with a mock failing primary.
- **SC-003**: A 10-item JSONL batch submitted via the batch API completes and returns all 10
  output records with matching `custom_id` values.
- **SC-004**: Per-key spend reported by the admin API is within 5% of expected cost calculated
  from token counts and published model pricing.
- **SC-005**: A key with `max_budget_usd: 0.01` stops accepting requests once spend exceeds $0.01;
  verified by spend record query and 429 response body containing `budget_exceeded`.
- **SC-006**: A `developer`-role key returns 403 on admin endpoints and succeeds on LLM endpoints;
  verified by integration tests covering both paths.
- **SC-007**: Audio transcription and image generation requests return the backend's response
  byte-for-byte; verified by integration tests comparing raw response bodies.
- **SC-008**: All existing tests (~555) continue to pass after every tier's changes are merged.
- **SC-009**: `cargo clippy -- -D warnings` and `cargo fmt --check` pass on the final codebase.

---

## Assumptions

- Redis is an optional dependency; in-memory cache is the default and works without Redis.
- Qdrant is an optional dependency for semantic caching; its absence disables that feature without
  breaking anything else.
- Batch processing delegates actual inference to the backend's batch API; anyllm-proxy does not
  run inference itself or manage a local job queue beyond status tracking.
- The pricing table ships as a static JSON file bundled with the binary; a live-fetch/auto-update
  mechanism is out of scope.
- OIDC and IP allowlisting are out of scope; only key-role-based access control is implemented.
- Cost tracking for streaming is best-effort: if the backend omits the terminal `usage` SSE event,
  cost is recorded as 0.0 for that request.
- Audio and image endpoints are passthroughs only; no format translation or model mapping is
  performed on those request/response bodies.
- Fallback chains operate at the backend level (e.g., OpenAI backend → Azure backend), not at the
  model-within-backend level.
- Budget period resets are checked at request time (not via background cron) for v1; a request
  arriving in the new period triggers the reset on first check.
- Reranking endpoints are out of scope for this milestone; the proxy does not translate reranking
  request/response formats across providers.
- Implementation proceeds in tiers: Tier 1 (caching, fallback, batch, cost tracking) must pass
  all tests before Tier 2 work begins.
