# Changelog

All notable changes to this project will be documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/).

**Before cutting a release:** update this file under the `[Unreleased]` header. Move those entries into a new `## [X.Y.Z] - YYYY-MM-DD` section, then push a `vX.Y.Z` tag. CI handles the rest (crates.io publish + GitHub Release).

---

## [Unreleased]

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

[Unreleased]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.10.1...HEAD
[0.10.1]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.9...v0.10.0
[0.9.9]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.8...v0.9.9
[0.9.8]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.7...v0.9.8
[0.9.7]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.6...v0.9.7
[0.9.6]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.5...v0.9.6
[0.9.5]: https://github.com/whit3rabbit/anyllm-proxy/releases/tag/v0.9.5
