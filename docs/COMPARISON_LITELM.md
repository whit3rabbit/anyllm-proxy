# anyllm-proxy vs litelm: Feature Comparison

anyllm-proxy is a long-running **HTTP gateway** in Rust (axum), translating between Anthropic Messages API and OpenAI-compatible backends, with virtual keys, cost tracking, admin UI, and deployable artifacts (static binary, Docker image, Debian package).

litelm is an in-process **Python library** that re-implements LiteLLM's `completion()` / `embedding()` call path (~2,300 LOC, two dependencies) inside your application. It is *not* a server — you `import litelm` and call `litelm.completion("openai/gpt-4o", messages=[...])`. By design it has no Router, no proxy, no caching.

These are different categories. anyllm-proxy is a deployable infrastructure component; litelm is an SDK dependency. The relevant question is **"is there anything in litelm's design that anyllm-proxy should adopt?"** — not "which is better."

> **Pinned reference:** [litelm v0.5.0](https://github.com/kennethwolters/litelm/releases/tag/v0.5.0) (2026-04-22). The repo moves quickly; re-pin and rewrite if any axis below drifts.

## Summary Table

| Feature Area | anyllm-proxy | litelm | Gap |
|---|---|---|---|
| Runtime model | Long-lived HTTP server (Rust/axum) | In-process Python library | **Different category** |
| Interface | `POST /v1/messages`, `POST /v1/chat/completions` (HTTP) | `litelm.completion("provider/model", ...)` (function call) | **Different category** |
| Translation surface | Anthropic ↔ OpenAI Chat Completions ↔ Bedrock (full) | OpenAI ↔ Anthropic ↔ Bedrock ↔ Cloudflare ↔ Mistral | **Advantage (litelm)** |
| LOC for translation layer | ~5–10k across `translator` + `client` crates | ~2,300 in a single package | **Advantage (litelm)** |
| Runtime dependencies (translation path) | 0 (pure Rust, no HTTP SDK) | 2 (`openai`, `httpx`); `anthropic` and `boto3` optional | **Advantage (litelm)** |
| Streaming SSE | Yes, state machine in `streaming_map.rs` | Yes (`stream_chunk_builder`) | **Parity** |
| Tool calling | Yes, with `#[serde(flatten)]` for unknown fields | Yes | **Parity** |
| Embeddings | Passthrough (`POST /v1/embeddings` only) | First-class `litelm.embedding()` | **Advantage (litelm)** |
| Text completions | Passthrough only (`POST /v1/completions`) | First-class `litelm.text_completion()` | **Advantage (litelm)** |
| OpenAI Responses API | Backend only (`OPENAI_API_FORMAT=responses`), not live-tested | First-class `litelm.responses()` | **Advantage (litelm)** |
| Mock responses | No | Yes | **Advantage (litelm)** |
| DSPy compatibility | No | Yes, all 7 execution paths verified live | **Advantage (litelm)** |
| HTTP server / daemon | Yes (axum, port 3000) | No (by design) | **Different category** |
| Virtual keys + immediate revocation | Yes (SQLite) | No | **Different category** |
| Per-key RPM/TPM rate limiting | Yes (in-memory + Redis) | No | **Different category** |
| Cost tracking + budget enforcement | Yes (bundled pricing DB, webhook alerts) | No | **Different category** |
| Response caching (in-mem + Redis + semantic) | Yes (moka, Redis, Qdrant) | No | **Different category** |
| Admin UI + audit log | Yes (SPA on port 3001) | No | **Different category** |
| Load balancing across deployments | Yes (5 strategies incl. cost-based) | No | **Different category** |
| OpenTelemetry export | Yes (OTLP/HTTP, feature-gated) | No | **Different category** |
| Langfuse integration | Yes (named callback + env vars) | No | **Different category** |
| Multi-language SDKs | Rust (`anyllm_client`) | Python only | **Different category** |
| Distribution | Static binary, Docker image, `.deb` package | PyPI package (`pip install litelm`) | **Different category** |
| Test corpus size | ~1,100+ tests (Rust workspace) | 214 own tests + 56 ported litellm tests via `sys.modules` shim | Different methodology — see §3 |

---

## 1. What litelm does that anyllm-proxy does not

These are real advantages of litelm's design and worth naming honestly.

### 1.1 Minimum viable translation surface

litelm ships the same routing + format-translation behavior as LiteLLM in ~2,300 LOC and two dependencies. anyllm-proxy's translator crate alone is several thousand lines across `anyllm_translate` + `anyllm_client` + the Bedrock/Vertex/Gemini adapter code, with a much larger workspace pulling in axum, tokio, sqlx, moka, reqwest, and feature-gated Redis/Qdrant/OTEL crates.

**Why this matters:** the smaller surface is auditable in an afternoon. The maintainer attests upstream compatibility by reading LiteLLM's source; the comparison doc there reads "no actionable in-scope drift" instead of "we re-implemented from the OpenAPI spec." That is a fundamentally lighter correctness model.

**Why anyllm-proxy is bigger anyway:** the brief is different. The gateway needs HTTP serving, virtual key CRUD, admin UI, request logging, cost tracking, and budget enforcement because *those are the user-visible features*. The translation crate is the minority of the LOC; the rest is governance.

### 1.2 Library ergonomics for application code

`litelm.completion("openai/gpt-4o", messages=[...], stream=True)` inside an existing Python application is dramatically simpler than standing up an HTTP gateway, configuring auth, and routing the request through `http://localhost:3000/v1/messages`. For teams building DSPy programs, agents, or batch scripts, the in-process model eliminates an entire deployable.

**This is intentional.** litelm's README opens with: *"litellm routes LLM calls across providers and translates between message formats. That core is buried under 100k+ LOC of proxy servers, caching layers, cost tracking, and dozens of features most users never touch. litelm extracts just the call path."* The positioning is the opposite of anyllm-proxy's.

### 1.3 First-class Responses API + embeddings + text completions

litelm exposes `litelm.responses()`, `litelm.embedding()`, `litelm.text_completion()` as **first-class entry points** with their own test coverage. anyllm-proxy handles these as passthrough endpoints (`/v1/completions`, `/v1/embeddings` are forwarded to the backend unchanged) and the OpenAI Responses API is a *backend* (`BACKEND=...` with `OPENAI_API_FORMAT=responses`) but is not live-tested per AGENTS.md.

**Implication:** if a user wants to call the Responses API and have it *translated* to an Anthropic backend (the value anyllm-proxy adds), that path is unwired and untested in anyllm-proxy.

### 1.4 Mock responses for tests

litelm ships a mock mode so tests can run without API keys. anyllm-proxy integration tests against live backends require real keys (`cargo test --test live_api -- --ignored --test-threads=1`). The fixture-based golden tests in `crates/translator/tests/` and `crates/proxy/tests/` cover translation logic without hitting the network, but the proxy layer itself is only exercised against real OpenAI/Anthropic.

---

## 2. What anyllm-proxy does that litelm does not

This section is not a defense — it's the reason anyllm-proxy exists at all.

### 2.1 Gateway deployment model

anyllm-proxy is a deployable artifact. Three distribution channels today:
- Static binary (`cargo build --release -p anyllm_proxy`) — single executable, no runtime deps
- Docker image (`followthewhit3rabbit/anyllm-proxy`, multi-arch)
- Debian package (`cargo deb -p anyllm_proxy`)

litelm is a `pip install` dependency. That is appropriate for its use case; it is not appropriate when you want a shared translation tier in front of many client applications.

### 2.2 Multi-tenant governance

This is the entire reason the proxy exists. From the existing COMPARISON_LITELLM.md:

- Virtual key CRUD via admin API, immediate revocation (SQLite + DashMap cache)
- Per-key RPM/TPM (in-memory sliding window; Redis-backed distributed with `--features redis`)
- Per-key budget enforcement with daily/monthly reset, 429 `budget_exceeded`
- Spend alerts at 80/95/100% via webhook
- RBAC (admin/developer roles)
- IP allowlisting with CIDR ranges and `X-Forwarded-For`
- Audit log of key CRUD + config changes
- LiteLLM `config.yaml` compatibility for migration

litelm explicitly lists all of these in its "what's out" table.

### 2.3 Cross-deployment routing

5 strategies (round-robin, least-busy, latency-EWMA, weighted, cost-based) across multiple deployments of the same model name, parsed from `router_settings.routing_strategy` in LiteLLM configs. Cost-based routing uses the bundled `model_pricing.json` to pick the cheapest deployment per token. litelm has no deployment concept — it routes to one model identifier per call.

### 2.4 Cost observability + response caching + OTel

Three features that are non-trivial to add to an in-process library:
- Per-request `x-anyllm-cost-usd` header from bundled pricing DB
- Three-tier response cache (moka L1, Redis L2, Qdrant semantic)
- OTLP/HTTP export behind `--features otel` (any OTEL-compatible collector)

litelm's `success_callback` hook (added in v0.5.0) is the closest equivalent and is the design to watch — see §5.

### 2.5 Rust client SDK

`anyllm_client` (`ClientBuilder`, `ToolBuilder`, typed streaming events) is published to crates.io independently of the proxy. A Python SDK would be the rough equivalent and is not on litelm's roadmap.

---

## 3. Where they agree

This is the section that matters most for positioning — anything below is **not a differentiator** and should not appear in roadmap discussions.

- `provider/model` string convention (e.g. `"openai/gpt-4o"`, `"anthropic/claude-sonnet-4-5"`, `"bedrock/anthropic.claude-3-sonnet"`)
- Streaming SSE with chunked content deltas
- Tool calling with function name + JSON-string arguments
- `reasoning_content` ↔ Anthropic thinking blocks (DeepSeek/Qwen support in both)
- OpenAI-compat local-model support via `api_base` override (vLLM, Ollama, LM Studio)
- Mock-first testing philosophy (litelm mocks at the HTTP layer; anyllm-proxy uses golden JSON fixtures)
- Per-call `stream=True` parameter
- The OpenAI Responses API translation surface (litelm exposes it; anyllm-proxy has it wired as a backend only)

---

## 4. Decisions

### Adopt: nothing from litelm into anyllm-proxy proper

**Rationale:** anyllm-proxy's value proposition is the gateway/management layer, not the translation core. Adopting litelm's "smaller translation surface" framing would mean *removing* features (no virtual keys, no admin UI, no cost tracking) to compete on LOC count, which is the wrong axis. The translator crate already isolates the pure-mapping code in `anyllm_translate`; the rest of the workspace is governance by design.

### Adopt (deferred): study litelm's `success_callback` hook design for the proxy's audit log

litelm v0.5.0 added a single `success_callback` hook fired after a successful completion. It's the simplest possible extension point and the maintainer resisted the temptation to add per-event hooks (started, streaming-chunk, failed) early.

**Rationale:** anyllm-proxy's audit log today is built around admin events (key CRUD, config changes) and request-level metrics. A `success_callback`-style hook at the translator boundary — before the response goes to the gateway layer — would let ops code (cost accounting, alert dispatch, OTel span closure) be added without growing the translator. Worth borrowing the *shape* (one hook, post-completion, typed payload), not the *implementation*.

### Reject: replace the Rust workspace with a Python library

The reasoning is the same as the existing comparison against LiteLLM: a library can't enforce virtual keys, rate limits, or budgets across multiple client applications. litelm solves this by explicitly not having those features — which is fine for litelm's use case and wrong for anyllm-proxy's.

### Reject (deferred): add a Python SDK that uses litelm internally

Tempting because it would give anyllm-proxy users a Python path without reimplementing the OpenAI/Anthropic/Bedrock format translation. **Don't do it.** Three reasons:

1. **Translation logic must match the proxy's behavior exactly.** The proxy adds lossy-translation signaling (`x-anyllm-degradation`), mTLS, and Anthropic-format-specific handling (system role injection, synthetic tool IDs for local backends) that litelm does not implement. A Python wrapper around litelm would silently diverge.
2. **Provider catalog is generated from LiteLLM's snapshot** (`crates/providers/src/providers/litellm_snapshot.rs`). The Rust crate owns provider metadata; a Python shim would need a parallel source of truth.
3. **DSPy integration is litelm's wedge, not ours.** litelm has DSPy verified across all 7 execution paths. We would be starting from zero against an entrenched incumbent.

If users want a Python SDK, the right path is either `pip install openai` pointed at `http://localhost:3000/v1` (existing approach, works today) or a future native Python client that uses the proxy as the translation tier — not a fork of litelm's call path.

---

## 5. What litelm does that this comparison doesn't measure

Honest gap section, per the standard pattern.

- **litellm test-port shim.** 56 LiteLLM tests pass unmodified via `sys.modules` shimming. This is a strong claim about correctness that anyllm-proxy cannot match — the Rust workspace re-implements from the OpenAPI spec and OpenAI's manual spec (`docs/openai-spec-condensed.yaml`), not from porting LiteLLM's test corpus. litelm's correctness evidence is more directly upstream-attested.
- **Live DSPy integration.** 10 live DSPy integration tests, all 7 execution paths proven. anyllm-proxy has no DSPy surface at all.
- **Anthropic SDK optional dependency.** `pip install litelm[anthropic]` pulls in the official `anthropic` SDK for higher-fidelity native Anthropic calls. anyllm-proxy implements its own Anthropic HTTP client in `anyllm_client`.
- **Callback infrastructure for application code.** litelm's `success_callback` is the start of a plug-in surface; anyllm-proxy's callback story is webhook URLs and Langfuse (admin-side), not application-side.
- **Mock responses.** Useful for tests and offline development. anyllm-proxy users wanting the same have to point at a stub backend (e.g. local LLM) — no first-class mock mode.

---

## 6. Architectural difference table

| | anyllm-proxy | litelm |
|---|---|---|
| **Language** | Rust (stable 1.83+, edition 2021) | Python (3.10+) |
| **Distribution** | Static binary, Docker image, `.deb` | PyPI package (`pip install`) |
| **Process model** | Long-lived HTTP server (axum) | In-process library import |
| **Concurrency** | Tokio async runtime, moka caches, DashMap | Stdlib asyncio, sync calls block |
| **HTTP server** | axum 0.7 on `0.0.0.0:3000` | None |
| **HTTP client** | reqwest (rustls), custom Anthropic client for sigv4 | `httpx` for everything; `anthropic` and `boto3` optional |
| **State storage** | SQLite (`~/.anyllm/admin.db`), Redis (optional L2) | None (stateless per call) |
| **Config format** | TOML (native), LiteLLM YAML (compat), env vars | Function arguments + env vars |
| **Auth model** | Static API keys, virtual keys (SQLite), OIDC/JWT | Caller's responsibility (pass `api_key` per call) |
| **Provider catalog source** | Generated from LiteLLM `model_prices_and_context_window.json` | Hand-written; LiteLLM-attested for compat subset |
| **Pricing source** | `assets/model_pricing.json` (bundled, weekly CI refresh) | None |
| **Telemetry** | OTLP/HTTP (`--features otel`), tracing crate, admin `/metrics` | `success_callback` hook (v0.5.0+) |
| **Test corpus** | ~1,100+ Rust tests + golden JSON fixtures; live tests gated on `--ignored` | 214 own tests + 56 ported litellm tests via `sys.modules` shim |
| **Lines of code (translation only)** | ~5–10k across `translator` + `client` + adapters | ~2,300 single package |
| **External runtime dependencies** | 0 (Cargo features are compile-time only) | 2 (`openai`, `httpx`); +1 per extras (`anthropic`, `boto3`) |

---

## Prior art cross-reference

- [COMPARISON_LITELLM.md](COMPARISON_LITELLM.md) — primary positioning doc (gateway vs gateway)
- [docs/codedocs/architecture.md](codedocs/architecture.md) — anyllm-proxy's internal architecture reference
- [docs/codedocs/translation-pipeline.md](codedocs/translation-pipeline.md) — the pure-mapping translator crate
- [docs/proxy-architecture.md](proxy-architecture.md) — crate layout and data flow
- Upstream: [kennethwolters/litelm](https://github.com/kennethwolters/litelm) at v0.5.0 (2026-04-22)
- Inspirational upstream: [BerriAI/litellm](https://github.com/BerriAI/litellm) (litelm is a minimal subset; anyllm-proxy's provider snapshot is generated from this)

> Rewrite this document when any of the following change in litelm: provider coverage in §1.3, the callback/hook surface in §5, or the upstream attestation in their README. Pin bumps without behavioral change do not require a rewrite.
