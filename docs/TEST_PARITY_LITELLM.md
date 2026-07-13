# LiteLLM Test Parity for anyllm-proxy

## Overview

anyllm-proxy is a specialized **protocol translator** (Anthropic <-> OpenAI format mapping) and proxy in Rust.
LiteLLM is a broad **AI gateway** in Python with 100+ provider backends, enterprise governance, and extensive
integration testing. These are different categories; porting every test is not the goal.

| Metric | LiteLLM | anyllm-proxy |
|--------|---------|--------------|
| Language | Python | Rust |
| Test framework | pytest | built-in `#[test]`, `rstest`, `test_case` |
| Total test files | 2,291 | 189 |
| Total test functions | ~31,129 | 1,193 |
| Integration tests | VCR-recorded live tests + unit tests | Mocked unit tests + fixture-based |
| Live API tests | Extensive (VCR cassettes) | Minimal (10 ignored tests) |

## Test Category Parity

Each category lists: litellm count -> ours count, portability, and priority.

P = Port (worth doing), S = Skip (not applicable), R = Reference (useful to read but can't port 1:1)

### 1. Format Translation / Provider Mapping

These are the closest to our `translator` crate.

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `llm_translation/base_llm_unit_tests.py` | 43 | 491 | **R** | Our translator has comprehensive mapping tests. Litellm tests are Python-specific (parameter validation via `get_optional_params`) |
| `llm_translation/test_openai.py` | 30 | inline | **R** | We cover OpenAI format via `openai/chat_completions/tests.rs` and mapping tests |
| `llm_translation/test_anthropic_completion.py` | 50 | inline | **P** | Missing: thinking block handling, tool streaming edge cases, metadata handling, citation streaming |
| `llm_translation/test_gemini.py` | 37 | inline | **R** | Our Gemini mapping tests (`gemini_message_map/tests.rs`, `gemini_streaming_map/tests.rs`) cover the core translation |
| `llm_translation/test_optional_params.py` | 78 | 0 | **P** | **Gap:** We have no `get_optional_params`-style validation. Tests like `test_anthropic_optional_params` (whitespace stop sequence dropped), `test_supports_system_message`, provider-specific param validation |
| `llm_translation/test_prompt_factory.py` | 75 | 0 | **S** | Tests prompt factory functions for different providers (Claude, Bedrock, Vertex). Our architecture is different -- we use struct-level serde transforms, not string templating |
| `llm_translation/test_prompt_caching.py` | 1 | 29 | **P** | We cover caching in `proxy/src/cache/tests.rs`. Our tests focus on response caching, not prompt caching token accounting |
| `llm_translation/test_deepseek_completion.py` | ~10 | 37 | **R** | **Covered.** Our `reasoning_content` <-> thinking mapping is tested in `message_map/tests.rs`, `streaming_map/tests.rs`, `reverse_message_map/tests.rs`, and `chat_completions/tests.rs`. Includes streaming lifecycle, tool call interleaving, thinking_blocks preference over reasoning_content, redacted thinking, and effective_text fallback. |
| `llm_translation/test_cohere.py` | ~30 | 0 | **S** | Cohere-specific translate tests. We don't target Cohere as a primary format; OpenAI-compat via OPENAI_BASE_URL |
| `llm_translation/test_groq.py` | ~10 | 0 | **S** | Groq-specific. Works via OPENAI_BASE_URL |
| `llm_translation/test_together_ai.py` | 1 | 0 | **S** | Provider-specific |
| `llm_translation/test_fireworks_ai_translation.py` | 8 | 0 | **S** | Provider-specific |

**Key gap:** We lack structured tests for optional parameter validation per provider. We rely on `#[serde(skip_serializing_if = "Option::is_none")]` which silently drops unsupported params -- that's by design, but we should verify behavior.

### 2. Proxy Server / Routes

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `proxy_unit_tests/test_proxy_server.py` | 51 | inline | **R** | Our proxy handler tests are spread across `server/routes/tests.rs`, `server/chat_completions/extensions/tests.rs`, and integration tests |
| `proxy_unit_tests/test_proxy_utils.py` | 69 | 0 | **R** | Tests address LiteLLM-specific utilities (Prisma client, metadata forwarding). Our architecture is Rust-native; not directly portable |
| `proxy_unit_tests/test_proxy_routes.py` | 11 | 5 | **P** | Route dispatch tests (`test_is_llm_api_route`, `test_anthropic_api_routes`, `test_get_request_route_*`). We have route dispatch in `server/routes/tests.rs` (4 tests) and `tests/route_dispatch.rs` (1 test). Still lacks edge case tests for URL parsing with base URLs, path injection, query params |
| `proxy_unit_tests/test_proxy_token_counter.py` | 24 | 14 | **P** | **Gap (narrowing):** Token counting tests. We now have 10 unit tests for `count_request_tokens_sync` (covering empty messages, system prompts, tool definitions, thinking blocks, tool results, multiple messages, image blocks) plus 4 integration tests in `compatibility.rs` (basic count, empty messages, tools, invalid body). Still missing provider-specific counting (GPT, vLLM, Vertex). |
| `proxy_unit_tests/test_proxy_exception_mapping.py` | 7 | 20 | **R** | **Covered.** Our `error_fixtures.rs` has 18 tests covering OpenAI-to-Anthropic translation for all major HTTP codes, chunk error classification, status boundaries, Azure auth, and context window exceeded. Plus 2 error-validation tests in `chat_completions.rs` for missing model/messages params. |
| `proxy_unit_tests/test_auth_checks.py` | ~40 | 30 | **R** | Our auth tests cover key generation, validation, and RBAC. Litellm tests cover end-user budget, model access control, team access. Different feature sets |
| `proxy_unit_tests/test_jwt.py` | ~20 | 0 | **S** | JWT auth is LiteLLM-specific; we support OIDC/JWT via separate mechanism |
| `proxy_unit_tests/test_proxy_config_unit_test.py` | 9 | 147 | **R** | Our config tests are more comprehensive (simple, multi, litellm, model_router formats). Litellm tests focus on file reading and OS env vars |
| `proxy_unit_tests/test_google_endpoint_routing.py` | 1 | 0 | **S** | Google-specific routing |
| `proxy_unit_tests/test_request_size_limit_middleware.py` | 3 | 0 | **S** | Request size limiting middleware -- not relevant to our architecture |
| `proxy_unit_tests/test_proxy_server_caching.py` | 1 | 29 | **R** | We cover caching more thoroughly |

### 3. Passthrough / Anthropic Messages

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `pass_through_unit_tests/test_anthropic_messages_passthrough.py` | 14 | 17 | **P** | **Gap:** Tests for Anthropic passthrough with streaming, bad requests, fallbacks, metadata tracking, extra headers, thinking blocks, Bedrock credential passthrough. We have `tests/compatibility.rs` for Anthropic endpoints but minimal coverage |
| `pass_through_unit_tests/base_anthropic_messages_prompt_caching_test.py` | 6 | 0 | **P** | **Gap:** Prompt caching token accounting tests (cache creation tokens, cache read tokens, streaming caching) |
| `pass_through_unit_tests/base_anthropic_unified_messages_test.py` | 4 | 0 | **P** | **Gap:** Unified message format tests (non-streaming, streaming, response format consistency) |
| `pass_through_unit_tests/test_passthrough_managed_ids.py` | 109 | 0 | **S** | Managed ID encoding/decoding system specific to LiteLLM's passthrough router. Not relevant to our architecture |
| `pass_through_tests/test_anthropic_passthrough.py` | 4 | 0 | **R** | Cost injection tests for Anthropic streaming |
| `pass_through_unit_tests/test_unit_test_anthropic_pass_through.py` | ~12 | 0 | **P** | Anthropic passthrough edge cases |

### 4. Streaming

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `local_testing/test_streaming.py` | 57 | 68 | **R** | Provider-specific streaming tests (Cohere, Azure, Gemini, Mistral, Bedrock, Ollama, Replicate, Vertex). We have generic streaming tests in `streaming_map/tests.rs` plus reasoning + tool call interleaving edge cases. |
| `proxy_unit_tests/test_unit_test_streaming.py` | 4 | 6 | **R** | **Covered.** Streaming passthrough test patterns: finish_reason stop streaming, backend HTTP error during streaming, tool call streaming in `chat_completions.rs`. |

### 5. OpenAI Endpoints Compliance

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `openai_endpoints_tests/test_openai_batches_endpoint.py` | 8 | 46 | **R** | **Covered.** Batch operations: create, list, cancel, status sync, VK isolation, invalid JSONL. Our `tests/batch_api.rs` has 8 proxy integration tests plus 38 batch engine unit tests covering queue operations, file storage, job lifecycle, and validation. |
| `openai_endpoints_tests/test_openai_files_endpoints.py` | 2 | 0 | **S** | File upload endpoints -- we delegate file handling to backend |
| `openai_endpoints_tests/test_openai_fine_tuning.py` | 1 | 0 | **S** | Fine tuning passthrough -- not supported |
| `openai_endpoints_tests/test_e2e_openai_responses_api.py` | 8 | 6 | **R** | **Covered.** Responses API mock tests: non-streaming, message content preservation, error handling. Plus 2 ignored live tests for real API verification. |

### 6. Batch Processing

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `batches_tests/` (9 files, 45 tests) | 45 | 38 | **R** | We cover batch in `batch_engine/src/queue/sqlite/tests.rs` and `proxy/tests/batch_api.rs`. Litellm adds: custom pricing, bedrock batch, hosted vLLM batch, rate limits, logging |
| `batches_tests/test_batch_custom_pricing.py` | ~5 | 0 | **P** | Custom pricing for batch jobs |
| `batches_tests/test_batch_rate_limits.py` | ~8 | 0 | **S** | Rate limit specific tests -- not directly ported |

### 7. Router / Load Balancing

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `router_unit_tests/` (19 files, 236 tests) | 236 | 0 | **S** | Extensive router tests: helper utils, budget limiter, cooldown handling, cost calculator, failovers, pattern matching, retries. These are Python-specific and deeply coupled to LiteLLM's Router class |
| `local_testing/test_router.py` | 87 | 0 | **S** | Core router tests |
| `local_testing/test_router_fallbacks.py` | 57 | 0 | **S** | Router fallback tests |
| `local_testing/test_router_retries.py` | 33 | 0 | **S** | Router retry tests |

### 8. Proxy Behavior / Management

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `proxy_behavior/management/` (46 files, 166 tests) | 166 | 0 | **S** | Extensive team/key/end-user management tests. LiteLLM's proxy management is more feature-rich (teams, organizations, SSO, SCIM). Our admin API is simpler |

### 9. Error Handling

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `local_testing/test_exceptions.py` | 50 | 0 | **R** | Exception handling tests. Different architecture |
| `proxy_unit_tests/test_proxy_exception_mapping.py` | 7 | 37 | **R** | **Covered.** Our `errors_map.rs` has 21 comprehensive unit tests (status mapping, error type round-trips, fixture deserialization, stream error classification). `error_fixtures.rs` now has 16 tests covering OpenAI-to-Anthropic translation for all major HTTP codes, chunk error classification, status boundaries, and edge cases. |

### 10. Logging & Observability

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `logging_callback_tests/` (35 files, 272 tests) | 272 | 0 | **S** | Callback/logging integration tests for 20+ platforms. Not directly portable; we use `tracing` + optional OTEL |
| `otel_tests/` (11 files, 41 tests) | 41 | 0 | **S** | OpenTelemetry-specific tests. Python OTel SDK tests; our OTEL is behind `--features otel` and uses `opentelemetry-rust` |

### 11. Guardrails

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `guardrails_tests/` (18 files, 189 tests) | 189 | 37 | **R** | LiteLLM has extensive guardrail integrations (Azure, OpenAI, Bedrock, custom hooks). We have `proxy/src/tools/guardrails/tests.rs` for forge-guardrails |
| `proxy_unit_tests/test_proxy_setting_guardrails.py` | 1 | 0 | **S** | Guardrail config tests |

### 12. End-to-End Tests

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `e2e/llm_translation/` (17 files, 37 tests) | 37 | 9 | **P** | **Key gap:** E2E tests for: audio speech, cache control, chat completions regression, custom pricing, DeepSeek reasoning, embeddings, image generation, messages, OCR, passthrough, rerank, responses, Vertex passthrough. We have 9 live tests (live_api.rs, live_responses.rs) |
| `e2e/llm_translation/test_passthrough_e2e.py` | 6 | 0 | **P** | Passthrough cost injection tests |
| `e2e/llm_translation/test_cache_control.py` | 2 | 0 | **P** | Cache control E2E tests |
| `e2e/llm_translation/test_deepseek_reasoning_e2e.py` | 3 | 0 | **P** | DeepSeek reasoning E2E tests |
| `proxy_e2e_anthropic_messages_tests/` (2 files) | 4 | 0 | **P** | Anthropic E2E passthrough: beta headers, Claude agent SDK |

### 13. Cost Management

| Category | LiteLLM | ours | Port? | Notes |
|----------|---------|------|-------|-------|
| `spend_tracking_tests/` (2 files, 13 tests) | 13 | 20 | **R** | We cover cost more thoroughly: model pricing DB, spend accumulation, budget enforcement |
| `proxy_unit_tests/test_proxy_server_spend.py` | 1 | 0 | **S** | Spend endpoint tests |
| `proxy_unit_tests/test_check_batch_cost.py` | 17 | 0 | **R** | Batch cost checking |

### 14. Provider-Specific (not portable)

| Category | LiteLLM | Port? | Reason |
|----------|---------|-------|--------|
| `test_litellm/llms/anthropic/` (8 files) | ~300 | **S** | Python SDK tests; we translate Anthropic to OpenAI |
| `test_litellm/llms/bedrock/` (9 files) | ~200 | **S** | Bedrock SDK tests; we have separate LLM backend |
| `test_litellm/llms/vertex_ai/` (16 files) | ~300 | **S** | Vertex-specific tests |
| `test_litellm/llms/openai/` (9 files) | ~100 | **S** | OpenAI SDK tests |
| `test_litellm/llms/azure/` (5 files) | ~100 | **S** | Azure-specific |
| `test_litellm/llms/gemini/` (5 files) | ~60 | **S** | Gemini-specific |
| `test_litellm/llms/mistral/` (2 files) | ~20 | **S** | Mistral-specific |
| `test_litellm/llms/databricks/` (5 files) | ~40 | **S** | Databricks-specific |

All provider-specific LLM SDK tests are **not portable** -- they test Python SDK wrappers for each provider.
Our architecture translates to OpenAI-compatible format and delegates to the backend. The translation
itself is tested in our translator crate.

### 15. Not Applicable (no equivalent in anyllm)

| Litellm Category | Tests | Reason |
|-----------------|-------|--------|
| `agent_tests/` | 11 | Agent SDK tests |
| `audio_tests/` | 24 | Speech/transcription -- different abstraction |
| `image_gen_tests/` | 57 | Image generation API tests |
| `mcp_tests/` | 147 | MCP server integration tests |
| `search_tests/` | 61 | Search/vector database tests |
| `vector_store_tests/` | 40 | Vector store management tests |
| `proxy_admin_ui_tests/` | 33 | Admin UI Selenium tests (we use SPA) |
| `documentation_tests/` | 3 | Doc validation tests |
| `benchmarks/` | 5 | Benchmark tests |
| `load_tests/` | 8 | Load/performance tests |
| `local_testing/test_completion_cost.py` | 187 | Cost calculation tests |
| `local_testing/test_caching.py` | 93 | Caching tests |

## Portability Summary

| Priority | Category | LiteLLM count | Port to Rust | Effort | Impact |
|----------|----------|--------------|--------------|--------|--------|
| **P0** | Anthropic passthrough E2E (streaming, thinking, caching, fallbacks) | 30 | ~12 Rust tests | Medium | High -- core protocol handling |
| **P1** | Token counting (multi-provider) | 24 | ~6 additional Rust tests | Low | Medium -- correctness |
| **P1** | Route dispatch edge cases (base URLs, injection) | 11 | ~5 Rust tests | Low | Medium -- security |
| **P2** | Prompt caching credential/token accounting | 7 | ~4 Rust tests | Low | Medium |
| **P2** | E2E cost injection for streaming | 6 | ~3 Rust tests | Low | Medium |
| **P3** | Optional param validation per provider | 78 | ~5 Rust tests | Medium | Low -- test structure diff |
| **P3** | Batch API E2E (terminal state sync, custom pricing) | 8 | ~2 Rust tests | Low | Low |
| ~~**R**~~ | Error mapping (provider error format handling) | 7 | Done (20 tests) | -- | -- |
| ~~**R**~~ | DeepSeek/Qwen reasoning content E2E | 10 | Done (37 tests) | -- | -- |
| ~~**R**~~ | Streaming test patterns | 4 | Done (6 tests) | -- | -- |
| ~~**R**~~ | OpenAI Responses API (non-live mock tests) | 8 | Done (6 mock + 2 live) | -- | -- |

## Priority Rationale

**P0 (Must port):** Core protocol behavior correctness. Litellm extensively tests Anthropic API passthrough with
streaming, thinking blocks, cache control, and error responses. These directly validate the same pipeline we
support. Missing coverage risks regressions.

**P1 (Should port):** Important correctness and error-handling areas. Token counting accuracy affects cost
tracking. Error mapping affects DX when providers return errors. Route dispatch edge cases affect security.

**P2 (Nice to port):** Streaming reliability and specific feature tests.

**P3 (Low):** Useful but lower impact. Optional param validation is structurally different in Rust
(serde-driven vs Python dict manipulation). Batch E2E needs live API key infrastructure.

## Test Patterns to Study

Some litellm test patterns worth referencing when writing new tests:

### Base class pattern for multi-provider coverage

`llm_translation/base_llm_unit_tests.py` defines `BaseLLMChatTest` with abstract tests that each
provider-specific test class inherits. Rust can approximate this with traits or test macros.

### Fixture-based golden tests

Litellm's VCR recording infrastructure (`_vcr_conftest_common.py`, `_openai_record_replay_proxy.py`)
records real API responses as fixtures. We use JSON fixture files in `fixtures/anthropic/` and
`fixtures/openai/`. Our approach is lighter weight and more deterministic, but adding live-recorded
fixtures for edge cases (e.g., real thinking blocks, real streaming sequences) would improve coverage.

### Error injection patterns

`test_anthropic_messages_passthrough.py` uses `test_anthropic_messages_streaming_with_bad_request` and
`test_anthropic_messages_router_streaming_with_bad_request` to test error handling during streaming.
We could add similar patterns using mocked HTTP handlers.

## Test Infrastructure Comparison

| Aspect | LiteLLM | anyllm-proxy |
|--------|---------|--------------|
| Mocking | `unittest.mock` (MagicMock, AsyncMock, patch) | Hand-rolled mock servers, fixture files |
| Live testing | VCR-recorded with cassette replay | `--ignored --test-threads=1` for live tests |
| Fixtures | `conftest.py` fixtures | JSON files in `fixtures/`, `#[test_case]` |
| Parameterization | `@pytest.mark.parametrize` | `rstest` + `#[test_case]` |
| Async | `pytest-asyncio` | `tokio::test` |
| Config | `conftest.py` + env vars | `test_config` helper setup |

## What We Already Cover Well

These areas have strong or sufficient coverage and do not need porting:

1. **Message format mapping** (Anthropic <-> OpenAI) -- 491 translator tests cover the core mapping
2. **Streaming state machine** -- 68 streaming tests cover chunk assembly, backpressure, finish reasons
3. **Tool calling** -- tools_map tests cover function calls, tool_choice, parallel tool calls
4. **Gemini format mapping** -- gemini_message_map + gemini_streaming_map tests
5. **Config parsing** -- 147 config tests across all formats (simple, multi, litellm, model_router)
6. **Cost tracking** -- 20 cost tests cover model pricing, budget enforcement
7. **Caching** -- 29 cache tests cover in-memory, Redis, semantic
8. **Auth** -- 30 auth tests cover key generation, RBAC, validation
9. **Virtual keys** -- integration tests for key CRUD, rate limiting
10. **Batch engine** -- 38 tests for queue operations, job lifecycle
11. **Fallback** -- 11 tests for fallback chain config, error classification, endpoint selection
12. **Thinking/repair** -- 17 tests for thinking block repair during streaming
13. **DeepSeek/Qwen reasoning content** -- ~37 tests across translator and proxy covering reasoning_content <-> thinking block mapping in both directions, streaming lifecycle, tool call interleaving, and effective_text fallback
14. **Error mapping** -- 18 error_fixtures.rs tests + 2 chat_completions validation tests covering all Anthropic error types, status mapping, fixture-based translation, stream error classification, Azure auth, and context window exceeded
15. **Token counting** -- 10 unit tests for count_request_tokens_sync + 4 integration tests covering basic counting, tools, system blocks, thinking blocks, and tool results
16. **OpenAI streaming passthrough** -- 6 tests in `chat_completions.rs` covering finish_reason stop, tool call streaming, backend HTTP errors during streaming, and malformed input validation
17. **OpenAI error format** -- Tests for missing model, missing messages, invalid JSON returning proper OpenAI error shapes
18. **OpenAI Responses API** -- 4 mock-based integration tests plus 2 ignored live tests covering non-streaming, content preservation, and error handling

## How to Run the Parity Assessment

```bash
# Run our full test suite
cargo test

# Run a specific test category
cargo test -p anyllm_translate
cargo test -p anyllm_proxy

# Run live API tests (needs API key)
OPENAI_API_KEY=sk-... cargo test --test live_api -- --ignored --test-threads=1

# Run golden fixture tests
cargo test -p anyllm_translate --test golden_fixtures
```

For litellm tests (reference only):
```bash
cd /tmp/litellm
python3 -m pytest tests/llm_translation/ -x -v --timeout 60
```
