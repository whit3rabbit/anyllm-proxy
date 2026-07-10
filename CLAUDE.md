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

Five-crate Cargo workspace: `providers` (metadata catalog), `client` (Anthropic HTTP client), `translator` (pure format mapping, no IO), `batch_engine` (job queue + webhook), `proxy` (axum HTTP server + admin UI). See [docs/proxy-architecture.md](docs/proxy-architecture.md) for crate details and data flow.

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

- **Managed backend fields cannot be cleared to NULL.** `ManagedBackendPatch` has no sentinel to distinguish "omitted" from "set to null". Once a field like `api_base` is set, it cannot be cleared via PATCH. UI should always send the current value in edit forms, not omit fields.
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
- **`reqwest::Error::is_timeout()` fires on read timeouts too.** A read timeout means the server already processed the POST. Only `is_connect()` is safe to retry on POST endpoints. The `retry_transport_errors` flag in `RetryPolicy` gates on `is_connect()` only by design — do not add `is_timeout()` back.
- **Two retry loops exist; keep them in sync.** The shared `anyllm_client::retry::send_with_retry_policy` (client crate) is canonical; the proxy's `backend/mod.rs::send_with_retry` and `client/anthropic_client.rs` both delegate to it (OpenAI/Gemini/Azure/Vertex/Anthropic). But `bedrock_client.rs` still has its own hand-rolled `for attempt in 0..=MAX_RETRIES` loop. A retry-policy change (backoff, quota fast-fail, status classification) must be applied to both or they diverge silently.
- **`anthropic::ErrorType::as_wire_str()` is the canonical snake_case stringifier.** Use it for error-event wire strings; do NOT round-trip through `serde_json::to_value(&et).as_str()...unwrap_or("api_error")` (silent fallback masks bugs). Adding a variant fails to compile until handled in the `match`.
- **Two unrelated `extra_headers` mechanisms exist; don't conflate them.** `HttpClientConfig.extra_headers` (`crates/client/src/http.rs`, applied once at client-construction time via `HeaderMap::insert` in `build_http_client`) really is last-writer-wins — push builder-level headers to the END of that Vec so they overwrite any caller-supplied duplicate; inserting at index 0 loses priority. But the PER-REQUEST `extra_headers: &[(&str, &str)]` param used in `anthropic_client.rs`'s `forward`/`forward_stream`/`forward_generic` applies via `reqwest::RequestBuilder::header()`, which APPENDS rather than replaces — calling it twice for the same header name sends two conflicting header lines upstream, not an override. Overriding a credential set earlier on that path (e.g. `ANTHROPIC_FORWARD_CLIENT_AUTH`) needs a real `Option<(&str, &str)>` parameter that skips the default `.header()` call entirely, not a Vec entry.
- **`Ipv4Addr::is_broadcast()` matches only `255.255.255.255`**, not directed subnet broadcasts like `10.0.0.255`. Directed broadcasts slip through SSRF filters when `allow_private=true`.
- **`2u64.pow(attempt)` overflows at attempt ≥ 64.** `backoff_delay` caps with `attempt.min(62)`. Any new backoff formula needs the same guard — `RetryPolicy::max_retries` has no upper bound.
- **SSE streaming: use `run_sse_task` in `crates/client/src/streaming.rs`.** New stream types must use this shared helper (BytesMut + `find_double_newline` loop + channel send). Implement as `FnMut(SseEvent<'_>) -> Vec<...>`. Do not duplicate the frame-reading loop.
- **SSE byte-framing is owned by `SseFrameBuffer` (`client/src/sse.rs`): `push(&bytes) -> Vec<Bytes>`, caps size before append.** All proxy stream loops use it EXCEPT `server/streaming.rs::observe_anthropic_sse_frames`, still on the raw `BytesMut` + `find_double_newline` path. Per-frame `data:` line parsing is NOT shared (duplicated ~7x).
- **Avoid `.clone()` before serialization when only one field changes.** Use `serde_json::to_value(req)` + field patch instead. `MessageCreateRequest` holds `Vec<Message>` + `serde_json::Map` — clone is O(content size).
- **Env-var tests must share `crate::config::ENV_TEST_LOCK`.** Tests that read/mutate process env (`ANTHROPIC_BASE_URL`, `OPENAI_API_KEY`, `*_API_KEY`, etc.) serialize on the crate-wide lock in `config/mod.rs` (poison-safe via `unwrap_or_else(|e| e.into_inner())`). A per-module `static Mutex<()>` does NOT serialize across modules within one `--lib` test binary, so cross-module env tests race and flake. Acquire the lock at the top of each such test.
- **Adding a `RuntimeConfig` field touches 6 sites; 2 are not compiler-caught.** Struct + `RuntimeConfigDefaults` (`admin/state.rs`) and 3 constructors (`SharedState::new_for_test`, `cost/mod.rs` test, `main_helpers/async_main/admin.rs`) fail to compile if missed — but the SQLite override-apply `match` (`main_helpers/async_main/admin.rs`) and the `delete_config_override` reset (`admin/routes/config.rs`) do NOT, so a new field silently won't persist or reset across restart unless added there too. (`main.rs` itself is now a thin bootstrap entry point post-refactor and holds neither site.)
- **`anthropic::ContentBlock`/`Tool` have no `cache_control` field and no `#[serde(flatten)] extra`.** An unrecognized block `"type"` collapses to a bare `#[serde(other)] Unknown` unit variant that loses every other field on deserialize, and re-serializes as just `{"type":"Unknown"}`. Never deserialize-then-reserialize a whole `MessageCreateRequest` when only one field changed (e.g. a repair/patch pass) — it silently drops `cache_control` prompt-cache breakpoints and any block/tool type the struct doesn't model yet, on every message, not just the one touched. Patch the raw `serde_json::Value` subtree instead.
- **Bulk edits that touch many struct-literal fields (e.g. adding a field to every call site of a struct) reliably break `cargo fmt --check`.** Run `cargo fmt` after any such edit, before considering the change done — CI's fmt gate fails even when `cargo build`/`clippy` are clean.

## Release Process

Publishing is fully automated via CI. There is no separate release script.

**To cut a release:**
1. Update `CHANGELOG.md`: move bullet points from `[Unreleased]` into a new `## [X.Y.Z] - YYYY-MM-DD` section.
2. Bump the version: `Cargo.toml` (workspace), `crates/client/Cargo.toml` (pinned directly), and inter-crate `version = "X.Y.Z"` refs in `crates/batch_engine/Cargo.toml` and `crates/proxy/Cargo.toml`.
3. Update `README.md`: the deb filename in the Linux install snippet is pinned (`anyllm-proxy_X.Y.Z-1_amd64.deb`). The `/releases/latest/download/` URL resolves to the latest release but still needs the exact current filename, so bump the version there.
4. Run `act -j test` locally before pushing — it runs the same `cargo audit` step as CI (fresh advisory-db pull), catching new RUSTSEC advisories before they show up as a CI failure post-push.
5. Commit and push to main.
6. Push a tag: `git tag vX.Y.Z && git push origin vX.Y.Z`

CI does the rest on tag push: builds binaries (Linux/macOS/Windows), packages debs, runs deb install tests, creates the GitHub Release using the CHANGELOG section for that version as the release body, uploads release assets, publishes all crates to crates.io in dependency order, and updates the Homebrew tap.

**Changelog discipline:** Every PR that changes user-visible behavior should add a bullet to `[Unreleased]` before merging. The release step is then just moving those bullets to the version section.

## Conventions

- **Adding a provider:** `crates/providers/src/providers/<name>.rs` (copy any stub) → add to `providers/mod.rs` + `registry.rs` → done. OpenAI-compat providers need no HTTP code.
- Test files live alongside source (`#[cfg(test)]`) and in `crates/proxy/tests/` for integration tests.
- Error types use `thiserror` derive macros.
- Fixture-based golden tests for translation correctness.
- **Model pricing is auto-updated.** `scripts/update_pricing.py` pulls from LiteLLM's `model_prices_and_context_window.json` and writes `assets/model_pricing.json` plus the packaged proxy copy at `crates/proxy/assets/model_pricing.json`. Run manually or via `.github/workflows/update-pricing.yml` (weekly, Monday 06:00 UTC). The proxy copy is embedded at compile time (`include_str!` in `crates/proxy/src/cost/mod.rs`); editing it requires recompile. Override at runtime with `MODEL_PRICING_FILE`.

## Active Technologies

- Rust stable (1.83+, workspace edition 2021)
- SQLite, Redis (optional rate-limit/cache), Qdrant (optional semantic cache, `--features qdrant`)

## CI / Workflow Validation

- **Validate workflows before pushing:** `brew install actionlint && actionlint .github/workflows/*.yml`
  Ruby YAML parser validates syntax only; actionlint catches GitHub Actions semantic errors.
- **secrets context in `if` conditions:** Not allowed at job or step level. Pass via `env:` and check
  in shell: `if [ -z "${SECRET}" ]; then echo "skipping"; exit 0; fi`
- **Heredocs in `run:` blocks:** Content must be indented to match the block level. Unindented
  heredoc content (col 0) breaks YAML parsing. Use `printf '%s\n' ...` or `{ echo ...; } > file`.
- **`gh release upload` requires the release to exist.** Add a `create-release` job before upload
  jobs: `gh release create "$TAG" --notes-file release_notes.txt || echo "already exists"`.
- **cargo publish exit 101** = version already exists on crates.io (not an error for re-runs).
  Pattern: `cargo publish -p FOO || { ec=$?; [ "$ec" -eq 101 ] && echo "already published" || exit "$ec"; }`

## npm / Frontend

- `@vitejs/plugin-react@4.7.0` declares peer deps only up to vite 7. With vite 8, use
  `npm ci --legacy-peer-deps` (in ci.yml AND Dockerfile stage that runs npm ci).

## crates.io Publish Order

anyllm_translate → anyllm_providers → anyllm_client → anyllm_batch_engine → anyllm_proxy
(sleep 30 between each for index propagation)

## Version Bumping

- **`crates/client/Cargo.toml` pins `version` directly** (not `version.workspace = true`). When bumping
  the workspace version, also update it there and all inter-crate `version = "X.Y.Z"` path deps.
  Quick check: `grep -r 'version.*0\.' crates/*/Cargo.toml Cargo.toml | grep -v "workspace"`
- Deb package version = Cargo workspace version, NOT the release tag. Keep them in sync.

## Code Intelligence

A **synrepo** MCP server is configured (`.mcp.json`) for structured codebase context. Start with `synrepo_orient`, use `synrepo_ask` for a cited context packet, `synrepo_find`/`synrepo_search` to locate code, `synrepo_impact` before edits, `synrepo_tests` before claiming done. Read raw source after synrepo narrows the target.

## References

- OpenAI API spec: https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml (very large, ~70k+ lines). Reference specific sections, do not load full spec.
- Endpoint inventory: [docs/ENDPOINTS.md](docs/ENDPOINTS.md)

## 2026-07-04: forge-guardrails integration finish (Eatahorse run)

Goal: finish integrating forge-guardrails (opt-in tool-call guardrails for local LLMs — lsp_first/quiet_command/write_payload_cap nudges, fingerprint dedup) into anyllm-proxy. The backend port itself (`crates/proxy/src/tools/guardrails.rs`, wiring into both tool loops, `SimpleConfig.build_tool_guardrail_config`, `FORGE_TOOL_CALL_POLICY` env fallback) was already done before this run; the run closed the verified gaps around it.

Completed (board cleared, 5 done / 1 dropped, 0 blocked, 11 iterations):
- **EH-0001**: Fixed `init_tool_engine` (`main_helpers/async_main/tools.rs`) so `FORGE_TOOL_CALL_POLICY` alone (no YAML `tool_execution` block, no `PROXY_CONFIG`) now initializes the guardrail engine — previously `tool_config.filter(|tc| tc.has_any())?` short-circuited to `None` before the env var was ever consulted, so the target local-LLM-env-var-only use case was silently dead. Env resolution now runs before the `has_any()` gate; YAML precedence over env is preserved.
- **EH-0002**: Documented (docs/CONFIG.md, docs/codedocs/configuration-and-modes.md) that the LiteLLM `model_list:` YAML format has no `tool_config` path at all — `tool_execution`/guardrails are silently ignored on that format; only simple-YAML `tool_execution.guardrails` or `FORGE_TOOL_CALL_POLICY` work. No LiteLLM parsing support was added (by design).
- **EH-0003**: Added the `[Unreleased]` CHANGELOG.md bullet for the guardrails feature.
- **EH-0005**: Added `tool_guardrail_mode` as a full `RuntimeConfig` field (struct + `RuntimeConfigDefaults` in `admin/state.rs`, all 3 constructors, SQLite override-apply arm, `delete_config_override` reset, GET/PUT config-route support), wired into `tools::resolve_runtime_guardrails` / `AppState::effective_tool_guardrails`, consumed by `handler.rs`, `routes/messages.rs`, and the streaming `tool_loop.rs`.
- **EH-0006**: Added the guardrail-mode `<select>` control to `admin-ui/src/tabs/settings/Settings.tsx`, wired to the existing config GET/PUT route.

Dropped: **EH-0004** ("optional admin UI + RuntimeConfig toggle") was split into EH-0005 (backend) + EH-0006 (UI) and dropped as a standalone card once superseded — not a dead end, just a decomposition artifact. No re-run action needed.

No cards were blocked this run.

Board invariants a rerun should keep honoring:
- Backend-then-UI split for RuntimeConfig-touching work (EH-0005 before EH-0006) so the UI card has a real field to bind to.
- Docs-only changes for format limitations (EH-0002) — explicitly do not build LiteLLM `tool_config` parsing support; that's an accepted gap, not a TODO.
- Env-var precedence rule: YAML-set guardrails config always wins over `FORGE_TOOL_CALL_POLICY`; do not invert this.
- Env var is the existing convention for backend toggles here — no second (e.g. CLI flag) mechanism was added for guardrail mode.
- New `RuntimeConfig` fields must touch the 6-site checklist (struct + defaults, 3 constructors, SQLite override-apply match in `main_helpers/async_main/admin.rs`, `delete_config_override` reset in `admin/routes/config.rs`) — note the checklist's override-apply site is `main_helpers/async_main/admin.rs`, NOT `main.rs` (main.rs is now a thin 167-line bootstrap entry point post-refactor; an earlier acceptance-check wording referencing `main.rs` for this was stale and was corrected in EH-0005's notes).

No manual-verification residuals were left behind: no card's `## Notes` contains a literal `residual:` line. (EH-0006's manual/browser acceptance item was itself discharged headlessly during the run — by driving the admin API directly, since no browser was available — rather than deferred; see its Notes for the exact curl sequence if a human wants to re-confirm via an actual browser click in `--webui`.)
