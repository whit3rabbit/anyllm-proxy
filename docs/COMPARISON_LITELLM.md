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
| `POST /v1/embeddings` | Passthrough (OpenAI/Azure/Vertex/Gemini/vLLM) | Yes | **Parity** |
| Virtual key management | Yes (SQLite-backed, immediate revocation) | Yes | **Parity** |
| Per-key rate limiting (RPM/TPM) | Yes (in-memory sliding window) | Yes | **Parity** |
| OpenTelemetry export | Yes (feature-gated, OTLP/HTTP) | 20+ integrations | Moderate gap |
| Cost tracking / budget enforcement | Yes (per-key, model pricing DB) | Yes | **Parity** |
| Response caching | Yes (in-memory moka, optional Redis) | Yes | **Parity** |
| Batch processing | Yes (OpenAI/Azure delegation) | Yes | **Parity** |
| Load balancing / fallback chains | Yes (YAML config, 5xx/429 failover) | Full | Near parity |
| Dynamic model management | Partial | Full | Moderate gap |
| RBAC | Yes (admin/developer roles) | Yes (+ OIDC) | Near parity |
| Audio, image endpoints | Yes (passthrough) | Yes | **Parity** |
| Semantic caching | Skeleton (qdrant feature flag) | Yes | Moderate gap |
| Reranking endpoints | No | Yes | Low (out of scope) |

---

## Detailed Gaps

### 1. Provider/Backend Coverage

**anyllm-proxy:** OpenAI (Chat Completions), OpenAI (Responses API), Vertex AI, Gemini, Azure OpenAI, AWS Bedrock, Anthropic passthrough.

**LiteLLM:** 100+ providers.

Providers that already work today via `OPENAI_BASE_URL` override (OpenAI-compatible):
- Groq, Together AI, Fireworks, Perplexity, Mistral, HuggingFace TGI, Ollama, vLLM

### 2. API Endpoints

anyllm-proxy accepts both **Anthropic-format** requests (`POST /v1/messages`) and **OpenAI-format** requests (`POST /v1/chat/completions`). The OpenAI endpoint translates internally through the Anthropic pipeline and returns OpenAI-format responses (streaming and non-streaming).

`POST /v1/embeddings` is supported as a transparent passthrough: the raw request body is forwarded to the backend and the response is returned unchanged. Supported backends: OpenAI, Azure OpenAI, Vertex AI, Gemini, and vLLM/HuggingFace models. Not mounted for the Anthropic passthrough or Bedrock backends.

Missing endpoints:
- `POST /v1/completions` — Legacy text completions
- `POST /v1/rerank` — Reranking (Cohere, etc.)

Newly added passthrough endpoints (no translation, forwarded to backend unchanged):
- `POST /v1/images/generations` — DALL-E, Imagen, etc.
- `POST /v1/audio/transcriptions` — Whisper / speech-to-text
- `POST /v1/audio/speech` — TTS
- `POST /v1/files` + `POST /v1/batches` + `GET /v1/batches/{id}` — Batch processing (OpenAI/Azure backends)

### 3. Authentication & Authorization

anyllm-proxy supports both static keys (`PROXY_API_KEYS` env var) and dynamic virtual keys managed via the admin API (`POST /admin/api/keys`). Virtual keys are stored in SQLite, cached in memory, and revocation takes effect immediately without restart. Per-key RPM rate limiting is enforced in the auth middleware.

anyllm-proxy now also provides:
- RBAC roles (admin, developer) enforced in auth middleware
- Per-key budget enforcement with daily/monthly period reset
- Developer keys blocked from admin endpoints (403)

LiteLLM additionally provides:
- OIDC/JWT validation
- IP allowlisting
- Read-only role

### 4. Load Balancing & Routing

anyllm-proxy supports multiple named backends via `PROXY_CONFIG` TOML and backend failover chains via `FALLBACK_CONFIG` YAML. When the primary backend returns 5xx, 429, or connection errors, the proxy silently retries against configured fallback backends. There is no load balancing between multiple instances of the same backend.

LiteLLM supports:
- Random shuffle, least-busy, latency-based, cost-based, and weighted routing
- Cross-provider fallback chains (retry on a different provider on failure)
- Redis-backed distributed state for multi-instance deployments

### 5. Caching

anyllm-proxy supports in-memory response caching (moka, configurable TTL and capacity) with optional Redis tier (`--features redis`). Per-request `cache_ttl_secs` override supported. `x-anyllm-cache` header reports hit/miss/bypass. Semantic caching via Qdrant is available as a skeleton behind `--features qdrant`.

LiteLLM supports in-memory, Redis, semantic (Qdrant/Redis), S3, and GCS caches with per-request TTL control.

### 6. Rate Limiting

anyllm-proxy enforces a global concurrency limit (default 100 concurrent requests) and per-key RPM limits via virtual keys (in-memory sliding window, no external dependencies). Upstream 429s are passed through. TPM tracking is recorded per-key for reporting.

LiteLLM enforces RPM and TPM limits per key, user, and team, with Redis-backed distributed tracking.

### 7. Cost Tracking & Budget Management

anyllm-proxy computes per-request USD cost from a bundled model pricing database (`assets/model_pricing.json`) and aggregates spend per virtual key. `x-anyllm-cost-usd` response header reports estimated cost. Admin API exposes per-key spend (`GET /admin/api/keys/{id}/spend`). Budget enforcement rejects requests when period spend exceeds `max_budget_usd` (daily/monthly reset).

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

anyllm-proxy supports async batch processing: upload JSONL via `POST /v1/files`, create batch via `POST /v1/batches`, poll status via `GET /v1/batches/{id}`, list via `GET /v1/batches`. Actual inference is delegated to the backend (OpenAI and Azure supported; other backends return 501). File and job metadata stored in SQLite.

LiteLLM supports batch processing across multiple providers.

### 11. Database & Persistence

anyllm-proxy uses SQLite for admin config overrides, request logs, virtual key management (with role, budget, spend tracking), batch file/job storage, and cost accumulation.

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
8. **`POST /v1/embeddings` passthrough** -- Transparent forwarding to OpenAI, Azure, Vertex, Gemini, and vLLM/HuggingFace backends; not mounted for Anthropic passthrough or Bedrock

## Completed Items (001-litellm-parity)

9. **Response caching** -- In-memory (moka) with optional Redis tier, per-request TTL, `x-anyllm-cache` header
10. **Backend fallback chains** -- YAML config, 5xx/429/connection-error failover, `x-anyllm-fallback-exhausted` header
11. **Batch processing** -- JSONL upload, batch create/poll/list, delegated to OpenAI/Azure backends
12. **Cost tracking** -- Bundled model pricing, per-key spend accumulation, `x-anyllm-cost-usd` header, admin spend endpoint
13. **Budget enforcement** -- Per-key max_budget_usd with daily/monthly period reset, 429 budget_exceeded
14. **RBAC** -- Admin/developer roles, developer keys blocked from admin endpoints
15. **Audio passthrough** -- Transcription and text-to-speech endpoints
16. **Image passthrough** -- Image generation endpoint
17. **Semantic caching skeleton** -- Qdrant-backed, behind `--features qdrant` feature flag

## Remaining Gaps

- OIDC/JWT authentication
- Reranking endpoint (`POST /v1/rerank`)
- Legacy text completions (`POST /v1/completions`)
- Multi-instance distributed rate limiting (Redis-backed)
- Full semantic caching implementation (embedding + vector search)
