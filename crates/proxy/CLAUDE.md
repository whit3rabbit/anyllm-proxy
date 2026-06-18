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

- **Admin UI requires a flag.** `--webui`/`--admin` or `WEBUI=1`/`ADMIN=1`. Without it, only the proxy starts.
- **Auth defaults to reject-all.** No `PROXY_API_KEYS` and no `PROXY_OPEN_RELAY=true` => every request is 401.
- **CSRF tokens are one-time-use.** Fetch a fresh one from `GET /admin/csrf-token` before each admin POST/PUT/DELETE. Scripts must too.
- **Docker admin needs `ADMIN_BIND=0.0.0.0`** — default 127.0.0.1 is unreachable from outside the container.
- **`OPENAI_API_KEY` takes precedence over provider keys for stub backends.** With `BACKEND=groq` but `OPENAI_API_KEY` set globally, the OpenAI key gets sent to Groq. Unset it when switching to a stub provider.
- **`BACKEND=sagemaker` panics at startup** (`ProviderProtocol::Custom` -> `resolve_backend()` None). Use `BACKEND=bedrock` for AWS-hosted Anthropic.
- **Adding a `RuntimeConfig` field touches 6 sites; 2 are not compiler-caught.** The struct + `RuntimeConfigDefaults` (`admin/state.rs`) and 3 constructors fail to compile if missed, but the SQLite override-apply `match` (`main.rs`) and `delete_config_override` reset (`admin/routes/config.rs`) do NOT — add there too or the field won't persist/reset across restart.
- **Managed backend fields cannot be cleared to NULL.** `ManagedBackendPatch` has no omitted-vs-null sentinel. Edit forms must resend the current value, not omit it.
- **`bedrock_client.rs` has its own retry loop** — keep it in sync with the canonical `anyllm_client::retry` (two loops total; `anthropic_client.rs` delegates to `retry.rs` and tracks automatically).
- **CPU-bound work (token counting) must use `tokio::task::spawn_blocking`.** `count_request_tokens_sync` in `server/token_counting.rs` is `pub(crate)` for reuse.
- **Env-var tests must share `crate::config::ENV_TEST_LOCK`** (a per-module `Mutex` does NOT serialize across modules in one test binary -> flakes).
- **Admin rate limiter resets on restart** (10 RPM/IP, in-memory). `set_admin_rpm()` overrides for tests.
- **Virtual keys use a global `OnceLock<DashMap>`** — integration tests share one `OnceLock` to avoid conflicts.
- **Passthrough routes:** reuse `passthrough_to_backend(...)` in `routes.rs` (Translate mode) or `anthropic_generic_passthrough` in `passthrough.rs` (Anthropic mode).
- **Admin UI npm:** with vite 8, `npm ci --legacy-peer-deps` (plugin-react peer dep caps at vite 7).
