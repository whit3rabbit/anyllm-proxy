# Feature Spec: LiteLLM Gap Fill + Rust Client Library Improvements

**Branch**: `20260325-120000-litellm-gap-fill`
**Date**: 2026-03-25
**Reference**: `docs/COMPARISON_LITELLM.md`

## Overview

Close the highest-value feature gaps between anyllm-proxy and LiteLLM while strengthening the
Rust client library (`anyllm_client`). The goal is not to replicate LiteLLM wholesale but to
eliminate blockers that prevent common OpenAI-native clients from using the proxy, add enterprise
backends (Bedrock, Azure), and provide a first-class Rust SDK experience.

## Problem Statement

1. **No OpenAI-format input**: Any client speaking `POST /v1/chat/completions` (OpenAI format)
   cannot use the proxy without an additional translation layer. This is the single largest
   adoption blocker.

2. **Missing enterprise backends**: AWS Bedrock (SigV4 auth) and Azure OpenAI (separate URL
   scheme, API version param, different auth header) are common enterprise targets with no support.

3. **Static API key management**: Adding/revoking proxy auth keys requires a process restart.
   No per-key metadata, expiry, or spend limits.

4. **No per-key rate limiting**: Global concurrency limit only. No per-key RPM/TPM enforcement.

5. **Weak Rust client library**: `anyllm_client` is a thin wrapper, not a first-class SDK.
   Missing: typed builder API, streaming ergonomics, tool-call helpers, retry configuration,
   and comprehensive documentation/examples.

6. **No OpenTelemetry export**: Observability is limited to stdout tracing and SQLite logs.
   No integration with Datadog, Honeycomb, or other OTEL collectors.

## Requirements

### Tier 1 (Must Have)

**R1. `POST /v1/chat/completions` input endpoint**
- Accept OpenAI Chat Completions format requests on the existing proxy listener
- Translate internally to Anthropic format, forward to configured backend, translate response back
- Support both non-streaming and streaming (`stream: true`) responses
- Return OpenAI-format responses (not Anthropic format)
- Set `x-anyllm-degradation` if features are dropped during reverse translation

**R2. AWS Bedrock backend**
- New `BACKEND=bedrock` option
- SigV4 request signing (AWS SDK for Rust or manual implementation)
- Support Claude-on-Bedrock model IDs (e.g., `anthropic.claude-3-5-sonnet-20241022-v2:0`)
- Map Anthropic request → Bedrock `InvokeModel` / `InvokeModelWithResponseStream`
- Required env vars: `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (+ optional `AWS_SESSION_TOKEN`)

**R3. Azure OpenAI backend**
- New `BACKEND=azure` option
- Base URL: `https://{resource}.openai.azure.com/openai/deployments/{deployment}`
- Auth header: `api-key: {key}` (not `Authorization: Bearer`)
- Query param: `api-version=2024-02-01`
- Required env vars: `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_DEPLOYMENT`, `AZURE_OPENAI_API_VERSION`

**R4. Virtual key management**
- Admin API endpoints: `POST /admin/keys`, `GET /admin/keys`, `DELETE /admin/keys/{id}`
- Per-key fields: id, created_at, expires_at (optional), description, spend_limit (optional)
- Keys stored in SQLite (extend existing admin DB)
- Key revocation takes effect immediately without proxy restart
- Backward-compatible: if `PROXY_API_KEYS` env var is set, those keys still work

**R5. Rust client library improvements**
- Typed builder API with `ClientBuilder` pattern
- Streaming API: `impl Stream<Item = AnthropicStreamEvent>` return type
- Tool-call helpers: typed `Tool`, `ToolChoice` builders
- Comprehensive rustdoc with examples
- Re-export all public types from crate root
- `anyllm_client` version bump to 0.2.0

### Tier 2 (Should Have)

**R6. OpenTelemetry export**
- Feature-gated with `features = ["otel"]` in `anyllm_proxy/Cargo.toml`
- Export spans to OTEL collector via `opentelemetry-otlp`
- Env var: `OTEL_EXPORTER_OTLP_ENDPOINT` (standard OTEL env var)
- Span attributes: request ID, model, backend, latency, token counts, degradation flags

**R7. Per-key rate limiting**
- RPM (requests per minute) and TPM (tokens per minute) limits per virtual key
- Add `rpm_limit` and `tpm_limit` fields to virtual key schema
- In-memory rate limit state (atomic counters with 60s sliding window)
- Return HTTP 429 with standard `retry-after` header when limit exceeded
- Requires R4 (virtual keys) to be complete first

### Out of Scope

- Response caching (Redis dependency, significant scope)
- Cross-provider fallback chains (router redesign required)
- Real batch processing (async job queue, out of scope)
- Cost tracking / pricing database
- RBAC / OIDC / SAML
- Audio, image, reranking endpoints

## Acceptance Criteria

1. `cargo test` passes (including new tests for each requirement)
2. `cargo clippy -- -D warnings` clean
3. `cargo fmt --check` clean
4. All new source files under 400 lines
5. `POST /v1/chat/completions` works with curl against a running proxy backed by OpenAI
6. Bedrock backend connects and returns a response (tested with `#[ignore]` live test)
7. Azure backend connects and returns a response (tested with `#[ignore]` live test)
8. Virtual key CRUD via admin API, key revocation verified without restart
9. `anyllm_client` rustdoc builds without warnings (`cargo doc --no-deps`)
10. OTEL spans visible in a local collector when feature flag is enabled (manual verification)
