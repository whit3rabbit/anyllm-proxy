# CLAUDE.md

## What This Is

**anyllm-proxy** is an API translation proxy in Rust. Accepts Anthropic Messages API and OpenAI Chat Completions requests, translates between formats, forwards to any supported backend (OpenAI, Azure, Vertex, Gemini, Bedrock, Anthropic passthrough), and translates back. Supports streaming SSE, tool calling, file/document blocks, virtual key management, batch API, and optional OpenTelemetry export.

## Build and Test

```bash
cargo build                          # build everything
cargo build --features otel          # with OpenTelemetry support
cargo test                           # ~1100+ tests, 10 ignored (live API)
cargo test -p anyllm_client          # client crate only
cargo test -p anyllm_translate       # translator crate only
cargo test -p anyllm_proxy           # proxy crate only
cargo test -p anyllm_providers       # provider/model catalog tests
cargo test health_endpoint           # single test by name
cargo test --test virtual_keys       # virtual key + rate limit integration tests
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

Run the proxy:
```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
# Listens on 0.0.0.0:3000, health at GET /health
```

Admin UI (separate port 3001):
```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy -- --webui
```

## Essential Env Vars

| Var | Purpose |
|-----|---------|
| `OPENAI_API_KEY` | Required for default backend |
| `BACKEND` | `openai` (default), `azure`, `vertex`, `gemini`, `anthropic`, `bedrock`, or any provider id from `crates/providers` (e.g. `groq`, `mistral`, `together_ai`) |
| `PROXY_CONFIG` | Path to config file (simple YAML, LiteLLM YAML, or TOML) |
| `PROXY_API_KEYS` | Comma-separated allowed keys (if unset and no `PROXY_OPEN_RELAY`, all requests rejected) |
| `PROXY_OPEN_RELAY` | `true` to accept any key (local dev only) |
| `RUST_LOG` | Tracing filter (e.g., `info`, `anyllm_proxy=debug`) |

Full env var reference: `crates/proxy/src/config/mod.rs` or [docs/ENV.md](docs/ENV.md).
LiteLLM env var aliases: search for `litellm_env_aliases` in `main.rs`.

## Not Fully Validated

- OpenAI Responses API backend (`OPENAI_API_FORMAT=responses`): wired up, not live-tested
- AWS Bedrock backend (`BACKEND=bedrock`): SigV4 signing + Event Stream decoding, not live-tested
- Azure OpenAI backend (`BACKEND=azure`): not live-tested
- Live integration tests: `cargo test --test live_api -- --ignored --test-threads=1` (needs real API key)

## Docker

Published as `followthewhit3rabbit/anyllm-proxy`. See Docker section commands:
```bash
docker compose up                    # uses .env file
# Smoke tests (no real key needed):
docker compose -f docker-compose.test.yml up -d --build
bash scripts/docker-smoke-test.sh
docker compose -f docker-compose.test.yml down -v
```

## Debian Package

```bash
cargo build --release -p anyllm_proxy
cargo deb -p anyllm_proxy --no-build --no-strip
```

After install: `sudo systemctl enable --now anyllm-proxy`, edit `/etc/default/anyllm-proxy`.

## Config Directory

Data lives in `~/.anyllm/` by default. Override with `ANYLLM_HOME`.
See [docs/CONFIG.md](docs/CONFIG.md) for lookup order, file layout, and config format docs.

## Architecture

Cargo workspace with five crates:

### `crates/providers` (lib: `anyllm_providers`)
Metadata-only catalog: no HTTP, no IO. `ProviderDef` (protocol, auth, env vars, LiteLLM prefix) and `ModelDef` (context window, capabilities). Registry functions in `registry.rs`. Add a new provider: create `providers/src/providers/<name>.rs`, register in `providers/mod.rs` and `registry.rs`. OpenAI-compatible providers route through the existing `OpenAIClient` automatically.

### `crates/client` (lib: `anyllm_client`)
Async HTTP client (Anthropic-in, Anthropic-out). `ClientBuilder`, `ToolBuilder`, `messages_stream()` returning `impl Stream`.

### `crates/translator` (lib: `anyllm_translate`)
Pure translation logic, no IO. Stateless `fn(A) -> B` mapping between Anthropic and OpenAI types.
- `anthropic/`: Anthropic Messages API types
- `openai/`: OpenAI types (Chat Completions + Responses API)
- `mapping/`: Conversion functions (message_map, tools_map, streaming_map, reverse_streaming_map, responses_*, warnings)
- `middleware/`: Request/response handler orchestrating translation

### `crates/batch_engine` (lib: `anyllm_batch_engine`)
HTTP-agnostic batch orchestration: job queue, file storage, webhook delivery.

### `crates/proxy` (bin: `anyllm_proxy`)
HTTP proxy on axum + reqwest:
- `server/`: Routes, middleware (auth, rate limit, request ID, size/concurrency limits), SSE streaming, passthrough handlers
- `backend/`: `BackendClient` enum dispatching to OpenAI/Azure/Vertex/Gemini/Anthropic/Bedrock with retry
- `admin/`: Admin server (localhost:3001), virtual key CRUD, model management, audit log, WebSocket live updates
- `admin-ui/`: React 19 + TypeScript SPA (Vite). Build: `cd crates/proxy/admin-ui && npm run build`

### Data Flow
```
Client (Anthropic or OpenAI format) -> proxy (axum)
  -> translator: input types -> mapping -> backend types
  -> backend: reqwest -> provider API
  -> translator: response types -> mapping -> client types
  -> proxy -> Client
```

## Key Design Decisions

- Translator crate is IO-free: pure `fn(A) -> B` mapping, testable without mocks.
- Tool call IDs pass through directly (Anthropic `tool_use.id` = OpenAI `tool_call.id`).
- OpenAI `arguments` is a JSON string; Anthropic `input` is a JSON object. Mapping layer handles serialization.
- Streaming uses a state machine (`streaming_map.rs`) with bounded channel (32) for backpressure.
- `ChatCompletionRequest` uses `#[serde(flatten)] pub extra: serde_json::Map` for unknown OpenAI fields. Only fields needing translation logic get explicit struct fields.
- `reasoning_content` maps bidirectionally to Anthropic thinking blocks (DeepSeek/Qwen support).
- Backoff jitter is deterministic (upper bound, not random) to keep tests predictable.
- Golden-file testing with JSON fixtures in `fixtures/anthropic/` and `fixtures/openai/`.

## Gotchas

- **`OPENAI_API_KEY` takes precedence over provider-specific keys for stub backends.** `config/mod.rs` tries `OPENAI_API_KEY` first, then falls back to `GROQ_API_KEY` / `MISTRAL_API_KEY` / etc. If `OPENAI_API_KEY` is set globally, it gets sent to Groq/Mistral/etc. even when `BACKEND=groq`. Unset it or clear it from `.anyllm.env` before switching to a stub provider.
- **`BACKEND=sagemaker` panics at startup.** Its `ProviderProtocol::Custom` makes `resolve_backend()` return `None`, triggering the "unknown backend" panic. Use `BACKEND=bedrock` for AWS-hosted Anthropic models instead.
- **Adding a passthrough route (Translate mode):** Reuse `passthrough_to_backend(&state, &headers, body, "/v2/path")` in `routes.rs` — it handles content-type forwarding and error mapping. The Anthropic mode equivalent is `anthropic_generic_passthrough` in `passthrough.rs` via `AnthropicClient::forward_generic`.
- **Header `&str` slices lifetime:** When building `&[(&str, &str)]` from `HeaderMap`, collect values into owned `String` locals first, then create references — the borrow checker rejects inline `.to_str()` in the slice.
- **CPU-bound work in handlers:** Token counting and similar CPU work must use `tokio::task::spawn_blocking`. `count_request_tokens_sync` (in `token_counting.rs`) is `pub(crate)` for reuse.
- **Gemini input actions:** `parse_model_action` in `gemini_input.rs` returns a `GeminiAction` enum. Extend it (not a bool) when adding new `:action` suffixes.
- **CSRF tokens are one-time-use.** Fetch a fresh token from `GET /admin/csrf-token` before each admin POST/PUT/DELETE. The SPA does this automatically; scripts must too.
- **Admin UI requires a flag.** Pass `--webui` or `--admin` (or `WEBUI=1`/`ADMIN=1` env). Without it, only the proxy starts.
- **Virtual key OnceLock in tests.** `set_virtual_keys` uses a global `OnceLock<DashMap>`. Integration tests in `crates/proxy/tests/virtual_keys.rs` use a shared `OnceLock` to avoid conflicts.
- **Auth defaults to reject-all.** Without `PROXY_API_KEYS` or `PROXY_OPEN_RELAY=true`, every request gets 401.
- **Admin rate limiter resets on restart.** 10 RPM per source IP, in-memory sliding window. `set_admin_rpm()` overrides for tests.
- **Docker admin needs `ADMIN_BIND=0.0.0.0`.** Default binds to 127.0.0.1 which is unreachable from outside the container.
- **PLAN.md references in source comments are stale.** Some files reference line ranges in a removed PLAN.md.

## Conventions

- **Adding a provider:** `crates/providers/src/providers/<name>.rs` (copy any stub) → add to `providers/mod.rs` + `registry.rs` → done. OpenAI-compat providers need no HTTP code.
- Test files live alongside source (`#[cfg(test)]`) and in `crates/proxy/tests/` for integration tests.
- Error types use `thiserror` derive macros.
- Fixture-based golden tests for translation correctness.
- **Model pricing is auto-updated.** `scripts/update_pricing.py` pulls from LiteLLM's `model_prices_and_context_window.json` and writes `assets/model_pricing.json`. Run manually or via `.github/workflows/update-pricing.yml` (weekly, Monday 06:00 UTC). The file is embedded at compile time (`include_str!` in `crates/proxy/src/cost/mod.rs`); editing it requires recompile. Override at runtime with `MODEL_PRICING_FILE`.

## Active Technologies

- Rust stable (1.83+, workspace edition 2021)
- SQLite, Redis (optional rate-limit/cache), Qdrant (optional semantic cache, `--features qdrant`)

## References

- OpenAI API spec: https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml (very large, ~70k+ lines). Reference specific sections, do not load full spec.
