# anyllm-proxy vs LiteLLM: Feature Comparison

anyllm-proxy is a specialized **protocol translator** (Anthropic API in, OpenAI-compatible backend out).
LiteLLM is a broad **AI gateway** focused on enterprise governance, cost control, and routing across 100+ providers.
These are different categories; not every gap is worth closing.

## Summary Table

| Feature Area | anyllm-proxy | LiteLLM | Gap |
|---|---|---|---|
| Protocol translation (Anthropic↔OpenAI) | Full | Partial | **Advantage** |
| Translation degradation warnings (`x-anyllm-degradation`) | Yes | No | **Advantage** |
| Local LLM compatibility (system role, synthetic IDs) | Yes | No | **Advantage** |
| mTLS backend support (PKCS#12) | Yes | No | **Advantage** |
| Single static binary, no runtime deps | Yes | No | **Advantage** |
| Provider backends | 7 (OpenAI, Vertex, Gemini, Azure, Bedrock, Anthropic, Responses) | 100+ | Moderate gap |
| `POST /v1/chat/completions` input | Yes (full, streaming + non-streaming) | Yes | **Parity** |
| Virtual key management | Yes (SQLite-backed, immediate revocation) | Yes | **Parity** |
| Per-key rate limiting (RPM/TPM) | Yes (in-memory sliding window) | Yes | **Parity** |
| OpenTelemetry export | Yes (feature-gated, OTLP/HTTP) | 20+ integrations | Moderate gap |
| Cost tracking / budget enforcement | No | Yes | Major gap |
| Response caching | No | Yes | Major gap |
| Batch processing | Stub (400) | Yes | Major gap |
| Load balancing / fallback chains | Basic | Full | Moderate gap |
| Dynamic model management | Partial | Full | Moderate gap |
| RBAC / OIDC auth | No | Yes | Moderate gap |
| Audio, image, reranking endpoints | No | Yes | Low (out of scope) |

---

## Detailed Gaps

### 1. Provider/Backend Coverage

**anyllm-proxy:** OpenAI (Chat Completions), OpenAI (Responses API), Vertex AI, Gemini, Azure OpenAI, AWS Bedrock, Anthropic passthrough.

**LiteLLM:** 100+ providers.

Providers that already work today via `OPENAI_BASE_URL` override (OpenAI-compatible):
- Groq, Together AI, Fireworks, Perplexity, Mistral, HuggingFace TGI, Ollama, vLLM

### 2. API Endpoints

anyllm-proxy accepts both **Anthropic-format** requests (`POST /v1/messages`) and **OpenAI-format** requests (`POST /v1/chat/completions`). The OpenAI endpoint translates internally through the Anthropic pipeline and returns OpenAI-format responses (streaming and non-streaming).

Missing endpoints:
- `POST /v1/completions` — Legacy text completions
- `POST /v1/images/generations` — DALL-E, Imagen, etc.
- `POST /v1/audio/transcriptions` — Whisper / speech-to-text
- `POST /v1/audio/speech` — TTS
- `POST /v1/rerank` — Reranking (Cohere, etc.)
- `POST /v1/messages/batches` — Currently stubbed; returns 400

### 3. Authentication & Authorization

anyllm-proxy supports both static keys (`PROXY_API_KEYS` env var) and dynamic virtual keys managed via the admin API (`POST /admin/api/keys`). Virtual keys are stored in SQLite, cached in memory, and revocation takes effect immediately without restart. Per-key RPM rate limiting is enforced in the auth middleware.

LiteLLM provides:
- Virtual key issuance via API (`POST /key/generate`) with per-key metadata, expiry, and spend limits
- Key revocation without restart
- RBAC roles (admin, developer, read-only)
- OIDC/JWT validation
- IP allowlisting

### 4. Load Balancing & Routing

anyllm-proxy supports multiple named backends via `PROXY_CONFIG` YAML, each served under its own path prefix (e.g., `/openai/v1/messages`, `/vertex/v1/messages`). There is no load balancing between multiple instances of the same backend.

LiteLLM supports:
- Random shuffle, least-busy, latency-based, cost-based, and weighted routing
- Cross-provider fallback chains (retry on a different provider on failure)
- Redis-backed distributed state for multi-instance deployments

### 5. Caching

anyllm-proxy has no caching. Every request hits the backend.

LiteLLM supports in-memory, Redis, semantic (Qdrant/Redis), S3, and GCS caches with per-request TTL control.

### 6. Rate Limiting

anyllm-proxy enforces a global concurrency limit (default 100 concurrent requests) and per-key RPM limits via virtual keys (in-memory sliding window, no external dependencies). Upstream 429s are passed through. TPM tracking is recorded per-key for reporting.

LiteLLM enforces RPM and TPM limits per key, user, and team, with Redis-backed distributed tracking.

### 7. Cost Tracking & Budget Management

anyllm-proxy has no cost tracking. No pricing database, no per-request USD calculation, no spend aggregation.

LiteLLM computes per-request USD cost from a built-in model pricing database and aggregates spend per key, user, and team with configurable hard caps.

### 8. Observability & Logging

anyllm-proxy provides:
- SQLite request log (7-day retention, latency percentiles via admin API)
- Request count metrics (`GET /metrics`)
- `x-anyllm-degradation` response header for lossy translation warnings
- `tracing` crate output (stdout, `RUST_LOG`)
- Optional OpenTelemetry OTLP export (`--features otel`): spans exported to any OTEL-compatible collector (Datadog, Honeycomb, Jaeger, Tempo, etc.) via `OTEL_EXPORTER_OTLP_ENDPOINT`

LiteLLM integrates with 20+ external observability platforms: Langfuse, Langsmith, OpenTelemetry (Honeycomb, Traceloop, OTEL collectors), Datadog, Sentry, Arize, and others. It also supports structured log export to DynamoDB, S3, GCS, and SQS.

Not present in LiteLLM: `x-anyllm-degradation` per-request degradation signaling.

### 9. Model Management

anyllm-proxy maps Haiku requests to `small_model` and Opus/Sonnet to `big_model`. Overrides persist to SQLite via the admin API.

The `/v1/models` response is a hardcoded list of 13 Claude model IDs with no context window or pricing metadata.

LiteLLM supports dynamic model addition and removal via API without restart, per-model pricing metadata, and enriched `/models` responses with token limits.

### 10. Batch Processing

`POST /v1/messages/batches` is stubbed and returns 400. LiteLLM supports batch processing across multiple providers.

### 11. Database & Persistence

anyllm-proxy uses SQLite for admin config overrides and request logs. There is no schema for virtual keys, users, teams, or spend records.

LiteLLM uses a full relational database (configurable backend) for key/user/team/spend storage.

---

## Completed Items (this release)

1. **`POST /v1/chat/completions`** -- Accept OpenAI-format input (streaming + non-streaming)
2. **AWS Bedrock backend** -- SigV4 auth, InvokeModel + InvokeModelWithResponseStream
3. **Azure OpenAI backend** -- Deployment-scoped URLs, `api-key` header auth
4. **Virtual key management** -- SQLite-backed CRUD, DashMap cache, immediate revocation
5. **Per-key RPM rate limiting** -- Sliding window enforcement, 429 + retry-after
6. **OpenTelemetry export** -- Feature-gated OTLP/HTTP, spans to any collector
7. **Rust client SDK v0.2.0** -- ClientBuilder, ToolBuilder, typed streaming

## Remaining Priority Order

**Tier 1 — Significant, broader scope:**
1. Response caching (in-memory + Redis) -- Reduces upstream cost and latency for repeated prompts
2. Cross-provider fallback chains -- Retry on alternate backend on failure
3. Real batch processing -- Async job queue behind `/v1/messages/batches`
4. Cost tracking -- Model pricing DB + per-request USD calculation + spend aggregation

**Tier 2 -- Enterprise/niche:**
5. RBAC / OIDC authentication
6. Audio, image generation, reranking endpoints
7. Semantic caching
8. Budget enforcement and spend alerts
