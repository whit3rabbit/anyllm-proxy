# Changelog

All notable changes to this project will be documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/).

**Before cutting a release:** update this file under the `[Unreleased]` header. Move those entries into a new `## [X.Y.Z] - YYYY-MM-DD` section, then push a `vX.Y.Z` tag. CI handles the rest (crates.io publish + GitHub Release).

---

## [Unreleased]

## [0.15.0] - 2026-07-15

### Changed
- Running `anyllm-proxy` with **no arguments** now starts the admin web UI alongside the
  proxy and auto-opens the default browser to the admin page (the zero-arg desktop
  default). Passing `--webui`/`--admin` still forces the UI on alongside other flags but
  no longer opens a browser (server-friendly). Passing any other argument (e.g.
  `--env-file`, `--redact-secrets`) keeps the proxy CLI-only, as before. `DISABLE_ADMIN=1`
  force-disables the admin server in all cases.
- Admin UI **Router** and **Routes** tabs merged into a single **Routing** tab with two
  subtabs: **Auto Router** (route by request shape) and **Model Routes** (named model
  aliases, load-balanced). The old `#/router` and `#/routes` URLs redirect to the matching
  subtab. No backend change; both config surfaces are unchanged.
- Admin UI theme controls moved from the sidebar into a **Settings > Display** section.
  Accent color and light/dark mode are now independent axes (mode layers on top of any
  accent color instead of "Light" being one of the color choices). Default accent is now
  Blue. Existing `anyllm.theme=light` localStorage values migrate to Blue + Light mode.
- Clearer admin-UI Settings help: the **thinking-block repair** and **forward client
  credential** runtime toggles are now grouped under an "Anthropic passthrough" sub-heading
  with rewritten plain-language descriptions and hover `?` tips explaining what each solves
  and when to enable it.
- Split large proxy modules into submodule directories for configuration, route APIs, passthrough handlers, streaming, and admin main helpers to improve codebase maintainability, and added `docs/TEST_PARITY_LITELLM.md`.
- Expanded Makefile build and test targets (such as `all-features`, `otel`, `qdrant`, `redis`, `optimizer`) and UI targets, and documented the admin UI build dist gotcha in `CLAUDE.md`.

### Added
- Claude Code tier router (opt-in, disabled by default): the **Routing > Auto Router** tab maps
  request tiers (Default, Background, Think, Long Context, Web Search, Image) each to a
  managed backend + model. The proxy classifies each `/v1/messages` request by its
  characteristics (image content, web-search tool, extended thinking, token count vs a
  configurable Context Threshold, haiku/background model) and routes to the matching tier,
  bypassing model-name routing. Also applied on the OpenAI `/v1/chat/completions` path
  (Long Context excluded there). Stored as one `router` config override; live-toggleable,
  no restart.
- Provider editor redesign: the provider detail modal is now a larger single-column form
  with an API-key show/hide toggle, an informational Models list (add + query-models
  discovery), and inline **Edit** of existing managed backends (not just create/delete).
- Dashboard sparklines and trend deltas: Admin UI dashboard stat cards now feature sparklines and trend percentage deltas, showing rolling activity trends (RPM, error rate, P50/P95 latency) for the last 24 retrieved metric snapshots.

## [0.14.1] - 2026-07-12

### Added
- RTK tool-output compression (`RTK_COMPRESS=true` / admin toggle): command-aware filtering
  of tool-result text (test/build/git/log output) using a catalog of 55 declarative filters
  ported from OmniRoute (MIT). New IO-free `anyllm_rtk` crate; deterministic and prompt-cache
  safe (`cache_control`-marked blocks are preserved byte-for-byte). Wired into the Anthropic
  passthrough (stream + non-stream) and OpenAI-translate paths, gated per-model via
  `RTK_MODELS` (empty = all). New `rtk_compress` / `rtk_models` runtime config + Settings UI.
- Release: GitHub Releases now include Linux (`x86_64`, `arm64`) `.tar.gz` and Windows
  (`x86_64`) `.zip` binary archives, alongside the existing macOS tarballs and `.deb`
  packages. The binaries were already built in CI but never packaged/attached.
- Opt-in prompt compression (`OPTIMIZER_MODE=off|shadow|live` / admin toggle):
  Frozen-Frontier Extractive Compression of long client-sent conversation history,
  applied for OpenAI Chat Completions, the Anthropic-Messages translate path, and the
  Anthropic passthrough path (`BACKEND=anthropic`) (client history only, never proxy
  tool-loop turns). `shadow` reports would-be savings without mutating the request;
  `live` compresses in place and, for Anthropic passthrough, places a `cache_control`
  breakpoint at the compression frontier on the wire (applied over raw bytes, so the
  breakpoint is not dropped by a typed round-trip). New
  IO-free `anyllm_optimize_core`/`anyllm_optimize_passes` crates; fails open on any
  error. New `optimizer_compressed_total` / `optimizer_messages_compressed_total` /
  `optimizer_removed_tokens_total` counters on `GET /metrics`. Optional LLMLingua-2 ONNX
  token-importance scorer behind the proxy's `optimizer-onnx` feature (never
  bundled/auto-downloaded): the model (~170MB, pinned to a sha256-verified HuggingFace
  artifact) is fetched on demand from the admin UI (Settings → Prompt compression →
  Download model) or the `optimize-model` CLI. The admin UI detects the model, gates the
  optimizer mode toggle on its presence, and offers a Download button when absent; the
  proxy loads the scorer eagerly at startup or lazily on the first live request after a
  download. Admin endpoints `GET`/`POST /admin/api/optimizer/model`; env `MODEL_URL` /
  `MODEL_SHA256` / `MODEL_CACHE_DIR` override the pin/cache location.

### Fixed
- Admin UI model discovery ("Query models"): fixed the discovery URL builder doubling
  `/v1` into `/v1/v1/models` for local providers whose catalog default base URL already
  ends in `/v1` (LM Studio, vLLM, etc.), and now trims trailing slashes off `api_base`
  before saving/discovering. Anthropic-native providers are now discoverable too:
  the request authenticates with `x-api-key` + `anthropic-version` (not a Bearer token)
  and reads the model name from `display_name` (Anthropic's field) as well as `name`.
  The Add-Backend form shows the exact URL "Query models" will hit and warns when
  discovery is unsupported for the provider protocol (Vertex/Gemini/Bedrock native).
- Security: Bedrock native routes (`POST /model/{modelId}/converse`,
  `/converse-stream`, `/invoke`, `/invoke-with-response-stream`) now enforce the
  virtual key's model allowlist. Previously these handlers skipped the
  `is_model_allowed` check that `bedrock_passthrough` and every other
  client-facing handler apply, so a model-scoped key could invoke any Bedrock
  `modelId` (model-scope bypass / cost-abuse). Found via a keyless 3gate scan.

## [0.13.0] - 2026-07-12

### Fixed
- Docker image build: the runtime stage copied and the entrypoint exec'd
  `anyllm_proxy` (underscore), but the binary was renamed to `anyllm-proxy`
  (hyphen) in 0.12.0, so the image build failed with "not found". Both now use
  the hyphenated name.

### Added
- Admin UI: enabled routes now show a ready-to-run `curl` snippet (proxy endpoint URL built from
  the new `proxy_port` in `/admin/api/status`, plus the route name as the `model`) with a one-click
  copy button. Routes have no unique URL; dispatch is by the `model` field, and the snippet makes
  that callable path obvious.
- Admin UI: Settings shows a live proxy status badge (running / unreachable), backed by a
  `proxy_running` TCP liveness check added to `/admin/api/status`, and a per-save toast confirming
  each setting is applied live with no restart.

### Changed
- Admin UI: selecting a provider now shows the full key/options form immediately instead of hiding
  it behind a "+ Add key" button (the API key is one of the fields). The old collapse toggle is gone;
  a "Reset" button clears the form.

### Added
- Routes now actually dispatch traffic. A request's `model` field selects a route (admin `routes` +
  `route_providers` tables), and the route picks one of its ordered managed backends by strategy
  (`failover` default, plus round-robin / least-busy / latency / weighted / cost). Previously the
  Routes tab was config-only and never routed. Implemented as a `RouteRouter` layer that reuses the
  existing `ModelRouter` strategy algorithms and sits ahead of LiteLLM model_list routing and the
  legacy default backend; installs with no routes are unaffected. Wildcard (`*`) and exact model
  globs are supported. When a model matches several routes the winner is chosen by route `position`
  (operator-set, lower wins), then exact-match over `*`, then route name.
- Per-route option overrides: guardrail mode, image/context compression (pxpipe) + model scope, and
  secret redaction can be set per route (nullable = inherit the global Settings value), plus a
  route-level on/off toggle and a global managed-backend on/off toggle. Admin UI: the route detail
  panel gains a strategy selector, tri-state option controls, a route enable toggle, and a
  backend-online toggle on each provider row; disabled routes also drop out of virtual-key route scope.
- Local LLM backends (LM Studio, Ollama, vLLM, llamafile, ...) now work end-to-end. SSRF protection
  is auto-relaxed to allow loopback + private/LAN IPs for managed backends whose provider is a local
  LLM server (detected from the catalog's default base URL); cloud-metadata IPs stay blocked. Admin UI:
  the provider popup gets an optional API Key field for local (`auth: none`) providers, a "Query models"
  button that discovers models from the configured endpoint, and shows the real backend error instead
  of a generic "Failed to create backend" banner.
- Admin UI Providers tab: favorite providers (heart toggle on each card) pinned to a top row and
  persisted server-side in SQLite (`provider_favorites` table, `GET/POST/DELETE /admin/api/favorites`).
  New sections group providers into Favorites, Local LLMs, and Free. Local-LLM providers
  (lm_studio, llamafile, vLLM, docker_model_runner, xinference, ollama, ...) are now surfaced in the
  catalog list and their loopback endpoint is pre-filled in the Add-key form (editable).

### Fixed
- Admin UI: the provider detail popup (add API keys per provider) now uses the shared centered modal
  with an internal scroll region, so the "Add key" form and its Create button can no longer render
  off-screen. Provider cards and the Add-Backend dropdown now show the provider id, so same-named
  providers (e.g. the several "AWS Bedrock" / "Google Vertex AI" entries) are distinguishable.
- Admin UI: Add Model's Backend field is now a dropdown of configured backends (no more free-text
  typos that wedged the button), and add failures surface an inline error instead of an endless
  spinner. Selecting a discovered model now also fills the Virtual Name.
- Admin UI: backend add/edit/delete now lives on the Backends page alongside live status (removed the
  duplicate "Managed Backends" section from Settings). The Settings "no backend configured" warning
  no longer fires when a managed backend exists — `/admin/api/status` now counts managed backends.
- Admin UI: created key value now has Copy and dismiss controls; Traffic's requests-by-route panel
  shows an empty state; the spend-limit field no longer clips its label; creating an unrestricted key
  (no spend/RPM limit) shows a warning; P50/P95/Window-failures/Window-cost metrics have help tooltips.

### Changed
- The compiled binary is now named `anyllm-proxy` (hyphen) directly, matching the installed
  name across Homebrew/deb/release archives. The crate/package name stays `anyllm_proxy`
  (`cargo install anyllm_proxy`, `cargo run -p anyllm_proxy` unchanged). README run commands
  updated to `anyllm-proxy`.
- Terminal logs default to human-readable format when stdout is a TTY, and JSON when piped
  (Docker/systemd) — previously always JSON. Override with `LOG_FORMAT=json` or `LOG_FORMAT=text`.
- `--webui`/`--admin` with no backend now prints a one-line hint pointing to the admin UI instead
  of the full backend cheat-sheet (which the UI already shows).

### Added
- On loopback admin binds (the default), the startup banner prints a ready-to-click admin URL with
  the token (`.../admin/?token=...`), so you no longer need to `cat ~/.anyllm/.admin_token`. Omitted
  on non-loopback binds (`ADMIN_BIND=0.0.0.0`) to avoid leaking the token into aggregated logs.
  The admin UI now reads `?token=` from that URL to log in automatically, then strips it from the
  address bar (previously the SPA ignored the query param and still showed the login prompt).
  The banner also prints the bare token on its own `Token` line for easy copy/paste (loopback only).

---

## [0.12.0] - 2026-07-05

### Added
- Opt-in text-to-image context compression (**experimental**), `PXPIPE_COMPRESS=true`. On the Anthropic
  passthrough path (`BACKEND=anthropic`), renders the stable system-prompt + tool-definition slab
  into a deterministic PNG glyph image and swaps it into the first user message, moving the caller's
  `cache_control` breakpoint onto the image so the imaged prefix still prompt-caches. Also images
  large `<system-reminder>` blocks and `tool_result` output (with a verbatim fact-sheet of paths/ids
  alongside, and paging that truncates oversized results under Anthropic's 100-image cap; `is_error`
  results are left as text). Optionally (`PXPIPE_HISTORY=true`, default off — highest cache-stability
  risk) collapses the old closed-tool-call conversation prefix into history image(s), keeping the recent
  tail as text; the collapse boundary is snapped to a message grid so the rendered PNG stays byte-stable
  across turns and keeps prompt-caching. Also covers the **translate path** (`BACKEND=openai`/`azure`/
  `vertex`/`gemini`-OpenAI): after the Anthropic→OpenAI mapping, images the static system/developer +
  tool-definition slab of the OpenAI Chat request for vision-capable, in-scope target models (gated by
  the same enable flag + scope + catalog vision check; GPT models are not in the default scope, so this
  is off unless the operator adds them). Saves input tokens on vision models that read imaged text reliably.
  Compression is observable via `GET /metrics` (`pxpipe_compressed_total` / `pxpipe_images_total` /
  `pxpipe_imaged_chars_total`), the admin dashboard (a compression stats row appears once it fires), and
  per-request `tracing` logs; the actual token savings surface in the upstream `usage.input_tokens`. Scope is a model allow-list (`PXPIPE_MODELS`
  CSV, default `claude-fable-5`); out-of-scope or non-vision models pass through untouched, and the
  transform fails open on any error. New IO-free `anyllm_pxpipe` crate holds the renderer + transform.
  The enable switch is opt-in via YAML (`pxpipe_compress: true` in simple config) or `PXPIPE_COMPRESS=true`
  env, and is live-toggleable from the admin UI/config API (`RuntimeConfig.pxpipe_compress`) without a
  restart — same cascade as `anthropic_thinking_repair`. Model scope (`RuntimeConfig.pxpipe_models`) is
  also runtime-editable: the admin Settings tab shows per-model checkboxes for the vision-capable Claude
  models and writes the scope CSV; `PXPIPE_MODELS` env seeds the default (`claude-fable-5`). Non-vision
  or out-of-scope models pass through untouched.

- Opt-in `ANTHROPIC_FORWARD_CLIENT_AUTH` for Anthropic passthrough: forwards the client's own
  incoming `x-api-key`/`Authorization`/`x-goog-api-key` credential upstream (renamed to `x-api-key`
  when it came in as `x-goog-api-key`, since Anthropic doesn't recognize that header name) instead
  of the operator's configured credential, for single-key/BYOK deployments (e.g. using Claude
  Code's own Pro/Max subscription OAuth token directly, no separate `claude setup-token` step).
  Only applies when the request authenticated via a static `PROXY_API_KEYS` entry or
  `PROXY_OPEN_RELAY`; virtual-key and OIDC-authenticated requests always use the operator's own
  credential regardless of the toggle. Applies uniformly to every Anthropic-kind backend in a
  multi-backend deployment (one shared runtime setting, like `ANTHROPIC_THINKING_REPAIR`) and is
  live-toggleable from the admin UI (**Settings**) / `PUT /admin/api/config` with no restart.
  Enabling it (at startup or live) is rejected when `PROXY_API_KEYS` has 2+ distinct entries and no
  open relay, since that combination would let different callers each redirect the upstream
  Anthropic credential.

- Opt-in Forge-style tool-call guardrails: advisory nudges for `lsp_first`, `quiet_command`, and
  `write_payload_cap` policies, plus fingerprint-based dedup of repeated tool calls. Configure via
  the simple-YAML `tool_execution.guardrails` key or the `FORGE_TOOL_CALL_POLICY` env var (the env
  var is ignored when `tool_execution.guardrails` is already set in YAML). Not available when using
  the LiteLLM-format config loader, which does not parse the `guardrails` key.

- Refreshed the LiteLLM provider/model catalog: added 7 new providers (`darkbloom`, `libertai`,
  `pinstripes`, `scaleway`, `tencent`, `tensormesh`, `tinyfish`) and corrected model/pricing drift
  across ~28 existing providers (missing models, stale `max_output_tokens`, capability flags).
  Removed the now-redundant hand-maintained `scaleway` legacy stub in favor of the generated
  snapshot entry. Refreshed `assets/model_pricing.json` to add `claude-sonnet-5`.

### Security
- Bumped `opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp` (0.31 -> 0.32) and
  `tracing-opentelemetry` (0.32 -> 0.33) behind the optional `otel` feature, fixing a Dependabot
  advisory in `opentelemetry_sdk` (unbounded memory allocation in W3C Baggage propagation,
  patched upstream in 0.32.1).

## [0.11.0] - 2026-07-04

### Added
- Opt-in `ANTHROPIC_FORWARD_CLIENT_AUTH` for Anthropic passthrough: forwards the client's own
  incoming `x-api-key`/`Authorization`/`x-goog-api-key` credential upstream (renamed to `x-api-key`
  when it came in as `x-goog-api-key`, since Anthropic doesn't recognize that header name) instead
  of the operator's configured credential, for single-key/BYOK deployments (e.g. using Claude
  Code's own Pro/Max subscription OAuth token directly, no separate `claude setup-token` step).
  Only applies when the request authenticated via a static `PROXY_API_KEYS` entry or
  `PROXY_OPEN_RELAY`; virtual-key and OIDC-authenticated requests always use the operator's own
  credential regardless of the toggle. Applies uniformly to every Anthropic-kind backend in a
  multi-backend deployment (one shared runtime setting, like `ANTHROPIC_THINKING_REPAIR`) and is
  live-toggleable from the admin UI (**Settings**) / `PUT /admin/api/config` with no restart.
  Enabling it (at startup or live) is rejected when `PROXY_API_KEYS` has 2+ distinct entries and no
  open relay, since that combination would let different callers each redirect the upstream
  Anthropic credential.

- Opt-in Forge-style tool-call guardrails: advisory nudges for `lsp_first`, `quiet_command`, and
  `write_payload_cap` policies, plus fingerprint-based dedup of repeated tool calls. Configure via
  the simple-YAML `tool_execution.guardrails` key or the `FORGE_TOOL_CALL_POLICY` env var (the env
  var is ignored when `tool_execution.guardrails` is already set in YAML). Not available when using
  the LiteLLM-format config loader, which does not parse the `guardrails` key.

- Refreshed the LiteLLM provider/model catalog: added 7 new providers (`darkbloom`, `libertai`,
  `pinstripes`, `scaleway`, `tencent`, `tensormesh`, `tinyfish`) and corrected model/pricing drift
  across ~28 existing providers (missing models, stale `max_output_tokens`, capability flags).
  Removed the now-redundant hand-maintained `scaleway` legacy stub in favor of the generated
  snapshot entry. Refreshed `assets/model_pricing.json` to add `claude-sonnet-5`.
- Security: bumped `opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp` (0.31 -> 0.32) and
  `tracing-opentelemetry` (0.32 -> 0.33) behind the optional `otel` feature, fixing a Dependabot
  advisory in `opentelemetry_sdk` (unbounded memory allocation in W3C Baggage propagation,
  patched upstream in 0.32.1).

---

## [0.10.1] - 2026-06-20

### Added
- OpenAI tool-call normalization: outbound tool-call IDs are rewritten to 9-digit sequential for providers that require it (Mistral, Codestral, OpenRouter), tools are sanitized for the Gemini/Vertex OpenAI shim (drops `strict`, sanitizes JSON Schema), and per-model tool capabilities (`tool_use`, `tool_choice`) gate unsupported requests with a clean 400.

### Fixed
- Forced `tool_choice` (`required` / named tool) is no longer rejected for self-hosted OpenAI-compatible providers (vLLM, LM Studio, llamafile, Triton, etc.): a provider-level `tool_choice` default is no longer treated as authoritative for an unknown model.
- `parallel_tool_calls=false` against Gemini/Vertex is now stripped with a degradation warning instead of returning a 400 when multiple tools are defined.
- `parallel_tool_calls` is included in the response cache key again: it changes model output on backends that honor it, so distinct values no longer collide on one cache entry.
- Streaming tool-call continuation deltas are no longer stamped with a synthetic `type:"function"`, which violated the OpenAI streaming contract on the passthrough path.
- Tool-policy provider quirks are now driven by the provider catalog instead of hardcoded id lists: the Gemini/Vertex OpenAI-shim sanitization keys off `ProviderProtocol` (so every Vertex/Gemini-shim provider is covered, not just two ids), and the "needs numeric tool-call IDs" trait is a catalog flag (`requires_numeric_tool_call_ids`).
- Admin UI key edit no longer wipes the enforced budget, TPM limit, model allowlist, and expiry on save; the spend-limit field now drives the enforced `max_budget_usd`.
- Admin UI managed-backend edit form now seeds existing values instead of starting blank (no-op saves / apparent config wipe).
- Admin UI now refreshes Settings/Env on `config_changed` websocket events from other sessions or the CLI.
- Admin UI request-log and observability backend filters are now populated (were empty, dead controls).
- Admin UI Keys tab now renders again: the `useKeys` hook unwraps the `{keys:[...]}` response instead of treating it as a bare array (the tab threw at runtime).
- Admin UI request-log and audit pagination now send `limit`/`offset` (the backend params) instead of `page`/`page_size`, so paging past the first page works.

---

## [0.10.0] - 2026-06-18

### Added
- Redesigned admin UI with admin API contract fixes.

### Changed
- Split oversized proxy modules into submodule directories (internal refactor).
- Dependency updates: `bytes`, `h2`, `syn`, `time`, `webpki-roots` (cargo update); dropped unused `wit-bindgen` tree. Admin-UI dev deps: `vite` and `@vitejs/plugin-react` bumped.

### Fixed
- Virtual-key accounting is now enforced in the Gemini native messages path (`BACKEND=gemini`), so usage is recorded consistently with other backends.

### Security
- Response cache is now scoped by auth key and backend, preventing cross-tenant/cross-backend cache leakage.
- Admin-UI dependency bumps: `dompurify` 3.4.11 and `js-yaml` 4.2.0.

---

## [0.9.9] - 2026-06-14

### Added
- `anthropic::ErrorType::TimeoutError` (`timeout_error`), matching Anthropic's documented `504` error type ([docs](https://platform.claude.com/docs/en/api/errors)).

### Fixed
- Gemini native backend (`BACKEND=gemini`, generateContent/streamGenerateContent) now retries `429`/`5xx` with backoff and honors `Retry-After`, matching every other backend. Previously it sent requests directly with no retry, so upstream rate limits failed immediately.
- `BackendError::api_error_status()` now includes the Anthropic passthrough variant. Without it, `status_code()` reported `500` and `error_kind()` reported `"unknown"` for Anthropic upstream errors (e.g. a real `429` was mis-tagged in logs/metrics).
- Fallback chain (`should_fallback`) now delegates to the shared `is_retryable` policy, so it covers `408` and all `5xx` (incl. `504`) instead of only `429/500/502/503`. The fallback and in-client retry layers can no longer disagree about what is retryable.
- HTTP `408`/`504` (timeouts) now map to Anthropic `timeout_error`, matching Anthropic's documented error codes (previously `504` was a generic `api_error`).
- `429` responses carrying OpenAI's `insufficient_quota` error code are no longer retried. Hard quota/credit exhaustion does not clear by waiting, so the error is surfaced immediately instead of wasting backoff cycles. Transient rate-limit `429`s still retry.
- Errors returned inside an HTTP `200` body by OpenAI-compatible gateways (notably OpenRouter, which puts the status in `error.code`) are now surfaced as a proper `ApiError` (with the upstream status, or `502` if absent) instead of a confusing deserialization failure. ([OpenRouter docs](https://openrouter.ai/docs/api/reference/errors-and-debugging))
- Gemini native streaming errors now surface the classified Anthropic error type derived from the upstream status (e.g. `rate_limit_error`, `permission_error`) instead of a hardcoded `api_error`.
- Mid-stream errors from OpenAI-compatible gateways (notably OpenRouter, which emits a chunk with a top-level `error` object and `finish_reason: "error"` once a `200` SSE stream has started) are now surfaced to the client instead of a silently truncated, apparently-successful response: Anthropic clients receive an `event: error` SSE frame and OpenAI-compatible clients receive an error chunk (`finish_reason: "error"` + `error` object). ([OpenRouter docs](https://openrouter.ai/docs/api/reference/errors-and-debugging), [Anthropic streaming docs](https://docs.anthropic.com/en/api/messages-streaming))
- Non-streaming responses where a `200` body carries a per-choice `finish_reason: "error"` (no top-level `error` envelope) are now surfaced as a `502` error instead of being returned as a truncated, apparently-successful completion. The streaming path already handled this; the non-streaming path now matches.
- `insufficient_quota` detection is now scoped to the structured `error.type`/`error.code` JSON fields instead of a raw substring scan, so a transient `429` whose message merely mentions the phrase (or echoes a prompt containing it) is no longer turned into a hard, non-retryable failure.
- The Anthropic passthrough and Bedrock retry loops now also fast-fail on `429` quota/credit exhaustion, consistent with the shared client retry loop (previously only the shared loop honored it).
- A re-translated mid-stream error type now round-trips: an Anthropic error event mapped to an OpenAI chunk and back (e.g. `overloaded_error`, `rate_limit_error`) recovers its classification instead of degrading to `api_error`.
- Numeric `error.code` values inside a `200` body that fall outside the `400..=599` HTTP-status range are now preserved on the surfaced error instead of being dropped.

---

## [0.9.8] - 2026-06-12

### Added
- `AnthropicMessagesClient` (`crates/client/src/anthropic_client.rs`): native Anthropic Messages API passthrough client. Sends `MessageCreateRequest` directly to the Anthropic API without format translation, with retry and SSE streaming.
- `RetryPolicy` struct in `crates/client/src/retry.rs`: explicit retry configuration with `max_retries` and `retry_transport_errors` fields. New `send_with_retry_policy` entry point; old `send_with_retry` shim retained.
- `ClientBuilder::retry_transport_errors(bool)`: opt-in flag to retry connect-only transport errors (off by default; POST endpoints are not idempotent).
- `ClientBuilder::extra_header(name, value)`: add static per-request headers (e.g. `HTTP-Referer` for OpenRouter).
- `HttpClientConfig`: new fields `ssrf_allow_loopback`, `ssrf_allow_private`, `extra_headers` for finer SSRF control and per-client headers.
- `ChatMessage::effective_text()`: coalesces `content` and `reasoning_content` into the first non-empty text string.
- `claude-fable-5` model in the Anthropic provider catalog (1M context, 128K output, extended thinking).
- `ChatCompletionResponse` and `ChatCompletionChunk`: `id`, `object`, `model`, and `choices` now default when absent, so lax local backends (llama.cpp, Ollama) that omit these fields no longer cause deserialization failures.

### Changed
- `build_http_client`: P12 identity loading is now gated on `#[cfg(feature = "native-tls")]` to avoid a dead-code error under the `rustls` feature.
- `openai_to_anthropic_response`: emits a `tracing::warn` when the response has no choices rather than silently producing empty content.
- Dependency updates: tokio 1.50 → 1.52, axum 0.8.8 → 0.8.9, hyper 1.8 → 1.10, rustls 0.23.37 → 0.23.40, dashmap 6.1 → 6.2, qdrant-client 1.17 → 1.18, and ~100 other transitive updates.

### Fixed
- `InternalError` now implements `Debug` manually, resolving a derived-`Debug` visibility issue.
- Streaming chunks with no `choices` array (usage-only final chunks from some gateways) are accepted without error.

---

## [0.9.7] - 2026-05-22

### Changed
- Version bump accompanying CI batch-build improvements.
- Hardened HTTP client used for provider model refresh (PR #20).

---

## [0.9.6] - 2026-04-XX

### Changed
- Provider catalog aligned with LiteLLM canonical IDs.

---

## [0.9.5] - 2026-03-XX

### Changed
- Dependency bumps (rand 0.8.5 → 0.8.6).

[Unreleased]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.15.0...HEAD
[0.15.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.14.1...v0.15.0
[0.14.1]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.13.0...v0.14.1
[0.13.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.10.1...v0.11.0
[0.10.1]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.9...v0.10.0
[0.9.9]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.8...v0.9.9
[0.9.8]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.7...v0.9.8
[0.9.7]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.6...v0.9.7
[0.9.6]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.5...v0.9.6
[0.9.5]: https://github.com/whit3rabbit/anyllm-proxy/releases/tag/v0.9.5
