# anyllm_proxy

The axum HTTP server: request routing, backend dispatch, auth, virtual keys, admin UI, cost tracking, caching, fallback. Ties the other crates together.

See root `../../CLAUDE.md` for workspace-wide commands, env vars, and the full gotchas list. This file covers proxy-internal specifics.

## Run / Test

```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy            # proxy only, :3000
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy -- --webui  # + admin UI :3001
cargo test -p anyllm_proxy
cargo test --test virtual_keys      # virtual key + rate-limit integration
cargo test --test live_api -- --ignored --test-threads=1    # needs real key
```

- **Built binary is `anyllm-proxy` (hyphen); Cargo package id is `anyllm_proxy` (underscore).** Run the built artifact as `target/debug/anyllm-proxy`; `cargo ... -p anyllm_proxy` uses the underscore. Bin-target unit tests need `cargo test -p anyllm_proxy --bins <name>` (default `-p` runs lib tests only).

## Layout

- `server/routes.rs` — main request routing; highest-churn file in the repo, change carefully.
- `main.rs` — startup, config wiring, env-alias resolution; also high-churn.
- `backend/` — one client per backend: `openai_client`, `gemini_client`, `anthropic_client`, `bedrock_client`. `mod.rs` has `resolve_backend()` + `send_with_retry`.
- `config/` — config loading (simple YAML, LiteLLM YAML, TOML), env aliases, model router, TLS, URL validation.
- `admin/` — admin API, auth, keys, spend, websocket feed. `state.rs` holds `RuntimeConfig`.
- `admin-ui/` — React SPA (Vite). Built separately, embedded at compile time.
- `batch/` — HTTP surface over `anyllm_batch_engine`.
- `cache/` — memory / redis / semantic (qdrant, `--features qdrant`).
- `cost/` — pricing + spend; embeds `assets/model_pricing.json` via `include_str!`.

## Gotchas (proxy-specific)

- **Admin UI defaults on for bare invocation.** Bare `anyllm-proxy` (no args) starts proxy + admin UI and auto-opens the browser (zero-arg default). `--webui`/`--admin` or `WEBUI=1`/`ADMIN=1` force it on with other args (no browser). Any other arg keeps it CLI-only. `DISABLE_ADMIN=1` force-disables. Single gate: `main_helpers::bootstrap::admin_enabled` (used by `main.rs` and `init_admin`); browser open only on `is_default_launch` via `main_helpers::browser::open`.
- **Runtime smoke-test in isolation.** Use a fresh `ANYLLM_HOME=$(mktemp -d)` + non-default `LISTEN_PORT`/`ADMIN_PORT`: the real `~/.anyllm` DB's persisted admin config overrides can hang `--webui` *before* the servers bind (stalls right after "applied config overrides from database"), and port 3000 is often held by other dev servers (giving false `200`s from something that isn't the proxy). `--redact-secrets`/`REDACT_SECRETS` also adds multi-second startup — allow more time or omit for quick checks.
- **`main_helpers` (bin-only) tests can't use `ENV_TEST_LOCK`.** It's `pub(crate)` in the lib crate, unreachable from the bin crate. For bin-only code that reads env, split the pure logic (e.g. `admin_requested(args)`) from the env read (`admin_enabled`) and unit-test the pure part with no env mutation, instead of trying to serialize on the lock.
- **Auth defaults to loopback-open.** No `PROXY_API_KEYS`, no `PROXY_OPEN_RELAY`, no virtual keys, no OIDC => loopback peers accepted, LAN/remote get 401. Gate is `no_auth_configured() && peer_is_loopback()` at the top of `validate_auth` (`middleware/auth.rs`); proxy is served with `into_make_service_with_connect_info` so `ConnectInfo` is present. `effective_auth_mode()` feeds `GET /admin/api/status` (`auth_mode`) and the admin UI banner.
- **CSRF tokens are one-time-use.** Fetch a fresh one from `GET /admin/csrf-token` before each admin POST/PUT/DELETE. Scripts must too.
- **Live admin-endpoint smoke:** run with `ADMIN_TOKEN=<32+ chars> ... --webui` (admin on :3001). GET needs `Authorization: Bearer $ADMIN_TOKEN`. POST/PUT/DELETE ALSO need CSRF: `GET /admin/csrf-token` with a cookie jar (`curl -c jar`), then resend with `-b jar` + `X-CSRF-Token: <token>` (header must equal the cookie). Missing/mismatched CSRF returns 403 before your handler runs.
- **`main_helpers` is bin-only** (declared in `main.rs`, NOT `lib.rs`). Library code (anything reached via `crate::` at runtime, e.g. `optimizer.rs`) cannot use `crate::main_helpers::bootstrap::*` — it won't compile. The data-dir/home helpers live in `crate::config::helpers::{resolve_data_dir, home_dir}`; use those from lib code.
- **Docker admin needs `ADMIN_BIND=0.0.0.0`** — default 127.0.0.1 is unreachable from outside the container.
- **`OPENAI_API_KEY` takes precedence over provider keys for stub backends.** With `BACKEND=groq` but `OPENAI_API_KEY` set globally, the OpenAI key gets sent to Groq. Unset it when switching to a stub provider.
- **`BACKEND=sagemaker` panics at startup** (`ProviderProtocol::Custom` -> `resolve_backend()` None). Use `BACKEND=bedrock` for AWS-hosted Anthropic.
- **Adding a `RuntimeConfig` field touches 6 sites; 2 are not compiler-caught.** The struct + `RuntimeConfigDefaults` (`admin/state.rs`) and 3 constructors fail to compile if missed, but the SQLite override-apply `match` (`main_helpers/async_main/admin.rs`) and `delete_config_override` reset (`admin/routes/config.rs`) do NOT — add there too or the field won't persist/reset across restart.
- **Managed backend fields cannot be cleared to NULL.** `ManagedBackendPatch` has no omitted-vs-null sentinel. Edit forms must resend the current value, not omit it.
- **`bedrock_client.rs` has its own retry loop** — keep it in sync with the canonical `anyllm_client::retry` (two loops total; `anthropic_client.rs` delegates to `retry.rs` and tracks automatically).
- **CPU-bound work (token counting) must use `tokio::task::spawn_blocking`.** `count_request_tokens_sync` in `server/token_counting.rs` is `pub(crate)` for reuse.
- **Env-var tests must share `crate::config::ENV_TEST_LOCK`** (a per-module `Mutex` does NOT serialize across modules in one test binary -> flakes).
- **Admin rate limiter resets on restart** (10 RPM/IP, in-memory). `set_admin_rpm()` overrides for tests.
- **Virtual keys use a global `OnceLock<DashMap>`** — integration tests share one `OnceLock` to avoid conflicts.
- **Passthrough routes:** reuse `passthrough_to_backend(...)` in `routes.rs` (Translate mode) or `anthropic_generic_passthrough` in `passthrough.rs` (Anthropic mode).
- **Admin UI npm:** with vite 8, `npm ci --legacy-peer-deps` (plugin-react peer dep caps at vite 7).
- **Cache key is a DENYLIST.** `should_include_cache_field` (`cache/mod.rs`) hashes every request-body field EXCEPT `stream`/`stream_options`/`_scope_auth`/`_scope_backend`/`user`/`parallel_tool_calls`/`metadata`. Add response-irrelevant fields here or they fragment the cache per-request. The old `CACHE_FIELDS` allowlist is gone.
- **Azure deployment URL is built once in `config/litellm/resolve_base_url`; `build_backend_config` passes it through.** `azure_deployment_from_model` strips route-group markers (`o_series/`, `gpt5_series/`); that list is the complete authoritative set.
- **Anthropic thinking-block repair (`thinking_repair/`, `ANTHROPIC_THINKING_REPAIR=true`) only wires up in `anthropic_passthrough` (`server/passthrough.rs`, `BACKEND=anthropic`'s `/v1/messages`).** It never touches messages before the last assistant one (prompt-cache prefix safety). In-memory `moka` store only, keyed on message id / thinking-block signature / tool_use id — restart loses it, feature fails open until a fresh response is recorded. All store keys are scoped by a `namespace` (backend name + virtual-key id) computed once per request in `passthrough.rs` — never call `ThinkingRepairStore`/`repair_request`/`record_response` without it, or one shared store across backends/tenants can cross-contaminate. Also toggleable live from the admin UI / `RuntimeConfig.anthropic_thinking_repair` (no restart) — `ThinkingRepairStore` is now always constructed for Anthropic backends regardless of the flag; the flag only gates the `repair_request`/`record_response`/`store.commit` call sites in `passthrough.rs` via `AppState::thinking_repair_enabled()`.
- **`server_advertised_tool_names` is hardcoded to an empty `HashSet` at every production call site** (`chat_completions/handler.rs`, `routes/messages.rs`, `chat_completions/stream/generic/tool_loop.rs`). `partition_tool_calls` therefore always routes every tool call to `pass_through`; `auto_exec`/`denied` are never populated in production today (tracked separately, `.eatahorse-integrate-forge-guardrails-opt-in-tool-c/tasks/ready/EH-0001-*`). Any new tool-loop mechanism (guardrails, audit, rate-limiting) must evaluate against the **post-partition `auto_exec`** set, never the raw `tool_calls` list — otherwise it silently answers on behalf of pass-through (client-owned) tool calls instead of returning them unresolved. See `tools::execution::partition_and_nudge` for the pattern.
- **`ThinkingRepairStore` (`thinking_repair/store.rs`) is 3 independent `moka::future::Cache` instances (`by_msg`/`by_sig`/`by_tool_use`) backing one logical record.** They evict independently even at the same capacity, so one index can miss while another still resolves the same message. Any new multi-index cache in this crate should either share one eviction clock or defensively re-verify via a second index before trusting a miss (see `repair.rs`'s `verified_via_owner_record` fallback).
- **`patch_repaired_body` (`thinking_repair/mod.rs`) fails open (forwards unrepaired bytes) if the last assistant message has a `cache_control` field or a block `"type"` outside `KNOWN_BLOCK_TYPES`.** Adding a new Anthropic content-block type to `ContentBlock` without adding it to `KNOWN_BLOCK_TYPES` doesn't break anything loudly — repair just silently stops firing for messages containing that block type.
