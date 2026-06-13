# Changelog

All notable changes to this project will be documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/).

**Before cutting a release:** update this file under the `[Unreleased]` header. Move those entries into a new `## [X.Y.Z] - YYYY-MM-DD` section, then push a `vX.Y.Z` tag. CI handles the rest (crates.io publish + GitHub Release).

---

## [Unreleased]

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

[Unreleased]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.8...HEAD
[0.9.8]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.7...v0.9.8
[0.9.7]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.6...v0.9.7
[0.9.6]: https://github.com/whit3rabbit/anyllm-proxy/compare/v0.9.5...v0.9.6
[0.9.5]: https://github.com/whit3rabbit/anyllm-proxy/releases/tag/v0.9.5
