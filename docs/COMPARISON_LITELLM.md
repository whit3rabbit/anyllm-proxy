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
| Per-key rate limiting (RPM/TPM) | Yes (in-memory + optional Redis distributed) | Yes | **Parity** |
| OpenTelemetry export | Yes (feature-gated, OTLP/HTTP) | 20+ integrations | Moderate gap |
| Cost tracking / budget enforcement | Yes (per-key, model pricing DB) | Yes | **Parity** |
| Response caching | Yes (in-memory moka, optional Redis) | Yes | **Parity** |
| Batch processing | Yes (OpenAI/Azure delegation) | Yes | **Parity** |
| LiteLLM config.yaml compatibility | Yes (model_list, env var aliases) | N/A | **Advantage** |
| Load balancing / fallback chains | Yes (round-robin, least-busy, latency-based, weighted, failover chains) | Full | **Parity** |
| Dynamic model management | Yes (model_list routing, admin API add/remove at runtime) | Full | **Parity** |
| RBAC | Yes (admin/developer roles, OIDC/JWT) | Yes (+ OIDC) | **Parity** |
| Audio, image endpoints | Yes (passthrough) | Yes | **Parity** |
| Semantic caching | Yes (Qdrant + embeddings, `--features qdrant`) | Yes | **Parity** |
| Reranking endpoints | Passthrough | Yes | **Parity** |

---

## Migrating from LiteLLM

### Config file

Set `PROXY_CONFIG=config.yaml` (or `LITELLM_CONFIG=config.yaml`) and point it at your existing LiteLLM config. The proxy parses `model_list`, `litellm_settings`, `router_settings`, and `general_settings`.

Supported `model_list` fields: `model_name`, `litellm_params.model` (provider/model format), `api_base`, `api_key`, `rpm`, `tpm`, `api_version`, `aws_access_key_id`, `aws_secret_access_key`, `aws_region_name`. Unknown fields are silently accepted (logged at debug level).

Supported providers in the `model` field: `openai`, `azure`, `vertex_ai`/`vertex`, `gemini`, `anthropic`, `bedrock`. Unknown providers are treated as OpenAI-compatible.

### Environment variables

LiteLLM env var names are accepted as aliases. The proxy checks for LiteLLM names at startup and maps them to anyllm equivalents (only when the target is not already set):

| LiteLLM env var | anyllm-proxy env var |
|---|---|
| `LITELLM_MASTER_KEY` | `PROXY_API_KEYS` |
| `LITELLM_CONFIG` | `PROXY_CONFIG` |
| `AZURE_API_KEY` | `AZURE_OPENAI_API_KEY` |
| `AZURE_API_BASE` | `AZURE_OPENAI_ENDPOINT` |
| `AZURE_API_VERSION` | `AZURE_OPENAI_API_VERSION` |
| `AWS_REGION_NAME` | `AWS_REGION` |

These env vars are the same in both projects (no alias needed): `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `ANTHROPIC_API_KEY`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `REDIS_URL`.

### Secret references in YAML

Both `os.environ/VAR_NAME` (LiteLLM syntax) and `env:VAR_NAME` (anyllm syntax) are supported in config values.

### What is NOT migrated

- `litellm_settings.callbacks` (Langfuse, DataDog, etc.) are ignored
- `router_settings.routing_strategy` values beyond simple-shuffle are ignored (round-robin + RPM-aware is always used)
- `general_settings.database_url` (PostgreSQL) is ignored; anyllm uses SQLite
- Team/user-level budgets (anyllm tracks per-key only)
- `litellm_settings.drop_params` is accepted but has no effect (anyllm already drops unsupported params via serde flatten)

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

Passthrough endpoints (no translation, forwarded to backend unchanged):
- `POST /v1/images/generations` — DALL-E, Imagen, etc.
- `POST /v1/audio/transcriptions` — Whisper / speech-to-text
- `POST /v1/audio/speech` — TTS
- `POST /v1/files` + `POST /v1/batches` + `GET /v1/batches/{id}` — Batch processing (OpenAI/Azure backends)

Passthrough endpoints (forwarded to backend unchanged):
- `POST /v1/rerank` — Reranking (Cohere, etc.)
- `POST /v1/completions` — Legacy text completions

### 3. Authentication & Authorization

anyllm-proxy supports both static keys (`PROXY_API_KEYS` env var) and dynamic virtual keys managed via the admin API (`POST /admin/api/keys`). Virtual keys are stored in SQLite, cached in memory, and revocation takes effect immediately without restart. Per-key RPM rate limiting is enforced in the auth middleware.

anyllm-proxy also provides:
- RBAC roles (admin, developer) enforced in auth middleware
- Per-key budget enforcement with daily/monthly period reset
- Developer keys blocked from admin endpoints (403)
- OIDC/JWT authentication (optional, via `OIDC_ISSUER_URL`): validates JWTs against JWKS with background key refresh
- IP allowlisting (optional, via `IP_ALLOWLIST` env var with CIDR ranges, `X-Forwarded-For` support)

LiteLLM additionally provides:
- Read-only role

### 4. Load Balancing & Routing

anyllm-proxy supports multiple named backends via `PROXY_CONFIG` TOML, backend failover chains via `FALLBACK_CONFIG` YAML, and LiteLLM-compatible `model_list` routing via `PROXY_CONFIG=config.yaml`. When using a LiteLLM config, multiple deployments of the same model name are load-balanced with configurable strategy (round-robin, least-busy, latency-based, or weighted), skipping deployments at their RPM limit. Per-deployment in-flight and latency EWMA tracking enables intelligent routing. Failover chains retry against configured backends on 5xx, 429, or connection errors.

LiteLLM supports:
- Random shuffle, least-busy, latency-based, cost-based, and weighted routing
- Cross-provider fallback chains (retry on a different provider on failure)
- Redis-backed distributed state for multi-instance deployments

Not yet in anyllm-proxy: cost-based routing strategy.

### 5. Caching

anyllm-proxy supports in-memory response caching (moka, configurable TTL and capacity) with optional Redis L2 tier (`--features redis`, SETEX-based with per-entry TTL). Per-request `cache_ttl_secs` override supported. `x-anyllm-cache` header reports hit/miss/bypass/semantic-hit. Semantic caching via Qdrant (`--features qdrant`) uses embedding-based similarity search: requests are embedded via the backend's embeddings endpoint, stored in Qdrant with cosine similarity, and matched against a configurable threshold (default 0.95). Collection auto-creation on first use.

LiteLLM supports in-memory, Redis, semantic (Qdrant/Redis), S3, and GCS caches with per-request TTL control.

### 6. Rate Limiting

anyllm-proxy enforces a global concurrency limit (default 100 concurrent requests) and per-key RPM/TPM limits via virtual keys (in-memory sliding window). With `--features redis` and `REDIS_URL`, rate limiting is distributed across instances using Redis sorted sets with atomic Lua scripts. On Redis failure, each instance falls back to local-only limiting. Upstream 429s are passed through.

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

anyllm-proxy supports two model routing modes:

1. **Simple mapping (TOML/env vars):** Maps Haiku requests to `small_model` and Opus/Sonnet to `big_model`. Overrides persist to SQLite via the admin API.
2. **LiteLLM model_list (YAML config):** Arbitrary model names routed to specific provider/model combinations. Multiple deployments per model name with per-deployment RPM/TPM limits and configurable routing strategy.

Dynamic model management via admin API: `POST /admin/api/models` to add deployments, `DELETE /admin/api/models/{name}` to remove, `GET /admin/api/models` to list. Changes take effect immediately without restart.

The `/v1/models` endpoint returns static Claude model IDs merged with models from the model_list config. Models added via admin API are also included.

LiteLLM supports dynamic model addition and removal via API without restart, per-model pricing metadata, and enriched `/models` responses with token limits.

### 10. Batch Processing

anyllm-proxy supports async batch processing: upload JSONL via `POST /v1/files`, create batch via `POST /v1/batches`, poll status via `GET /v1/batches/{id}`, list via `GET /v1/batches`. Actual inference is delegated to the backend (OpenAI and Azure supported; other backends return 501). File and job metadata stored in SQLite.

LiteLLM supports batch processing across multiple providers.

### 11. Database & Persistence

anyllm-proxy uses SQLite for admin config overrides, request logs, virtual key management (with role, budget, spend tracking), batch file/job storage, and cost accumulation.

LiteLLM uses a full relational database (configurable backend) for key/user/team/spend storage.

---

## Completed Items

1. **`POST /v1/chat/completions`** -- Accept OpenAI-format input (streaming + non-streaming)
2. **AWS Bedrock backend** -- SigV4 auth, InvokeModel + InvokeModelWithResponseStream
3. **Azure OpenAI backend** -- Deployment-scoped URLs, `api-key` header auth
4. **Virtual key management** -- SQLite-backed CRUD, DashMap cache, immediate revocation
5. **Per-key RPM rate limiting** -- Sliding window enforcement, 429 + retry-after
6. **OpenTelemetry export** -- Feature-gated OTLP/HTTP, spans to any collector
7. **Rust client SDK v0.2.0** -- ClientBuilder, ToolBuilder, typed streaming
8. **`POST /v1/embeddings` passthrough** -- Transparent forwarding to OpenAI, Azure, Vertex, Gemini, and vLLM/HuggingFace backends; not mounted for Anthropic passthrough or Bedrock
9. **Response caching** -- In-memory (moka) with optional Redis tier, per-request TTL, `x-anyllm-cache` header
10. **Backend fallback chains** -- YAML config, 5xx/429/connection-error failover, `x-anyllm-fallback-exhausted` header
11. **Batch processing** -- JSONL upload, batch create/poll/list, delegated to OpenAI/Azure backends
12. **Cost tracking** -- Bundled model pricing, per-key spend accumulation, `x-anyllm-cost-usd` header, admin spend endpoint
13. **Budget enforcement** -- Per-key max_budget_usd with daily/monthly period reset, 429 budget_exceeded
14. **RBAC** -- Admin/developer roles, developer keys blocked from admin endpoints
15. **Audio passthrough** -- Transcription and text-to-speech endpoints
16. **Image passthrough** -- Image generation endpoint
17. **Semantic caching skeleton** -- Qdrant-backed, behind `--features qdrant` feature flag
18. **Reranking passthrough** -- `POST /v1/rerank` forwarded to backend unchanged
19. **Legacy text completions passthrough** -- `POST /v1/completions` forwarded to backend unchanged
20. **OIDC/JWT authentication** -- Optional JWT validation via OIDC discovery, JWKS caching with background refresh
21. **Distributed rate limiting** -- Redis sorted sets with Lua scripts, fail-open fallback to local
22. **Redis L2 cache** -- SETEX-based response cache behind `--features redis`
23. **Semantic caching** -- Qdrant-backed embedding similarity search with collection auto-creation
24. **LiteLLM config.yaml compatibility** -- Accept LiteLLM config.yaml directly (`PROXY_CONFIG=config.yaml`), parse `model_list` with `provider/model` format, `os.environ/VAR` syntax, env var aliases (`LITELLM_MASTER_KEY`, `LITELLM_CONFIG`, `AZURE_API_KEY`, `AZURE_API_BASE`, `AZURE_API_VERSION`, `AWS_REGION_NAME`)
25. **Model-level routing** -- Round-robin + RPM-aware routing across multiple deployments per model name, cross-backend dispatch, lock-free atomic counters
26. **Advanced routing strategies** -- Least-busy (in-flight tracking), latency-based (EWMA), weighted round-robin; parsed from `router_settings.routing_strategy` in LiteLLM config
27. **Dynamic model management** -- Admin API (`POST/DELETE/GET /admin/api/models`) for runtime add/remove of model deployments without restart
28. **`/v1/models` enrichment** -- Endpoint merges static Claude models with model_list config entries and dynamically-added models
29. **IP allowlisting** -- `IP_ALLOWLIST` env var with CIDR ranges, `X-Forwarded-For` support via `TRUST_PROXY_HEADERS`
30. **Webhook callbacks** -- `litellm_settings.callbacks` webhook URLs and `WEBHOOK_URLS` env var; fire-and-forget POST on request completion

## Remaining Gaps

- **Routing strategies (cost-based):** LiteLLM supports cost-based routing; anyllm-proxy does not yet (round-robin, least-busy, latency-based, and weighted are supported).
- **LiteLLM named callbacks:** `litellm_settings.callbacks` entries like `"langfuse"` or `"datadog"` are not natively mapped (only webhook URLs are supported). Named integrations are logged as unsupported.
