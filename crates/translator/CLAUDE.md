# anyllm_translate

Pure format mapping between Anthropic Messages API, OpenAI Chat Completions, OpenAI Responses, and Gemini. **No IO** — every public fn is `fn(A) -> B`, testable without mocks or network.

See root `../../CLAUDE.md` for workspace-wide commands and conventions.

## Test

```bash
cargo test -p anyllm_translate
cargo test -p anyllm_translate --test golden_fixtures   # fixture-based correctness
```

## Layout

- `anthropic/`, `openai/`, `gemini/` — wire-format structs per API.
- `mapping/` — the actual translators. One file per direction/concern.
  - `message_map.rs`, `gemini_message_map.rs` — high-churn hotspots; change carefully.
  - `streaming_map.rs` + `reverse_streaming_map.rs` — SSE state machines.
  - `tools_map.rs`, `usage_map.rs`, `errors_map.rs`, `batch_map.rs`.
- `middleware/` — optional axum/reqwest glue, behind the `middleware` feature (off by default). Pure mapping stays IO-free; IO lives here only.
- `util/ids.rs`, `util/redact.rs`, `util/json.rs` — id generation, secret redaction, json helpers.

## Gotchas

- **Keep this crate IO-free.** No reqwest/tokio in non-`middleware` code. If you need a network call, it belongs in `proxy` or `client`, not here.
- **Tool call IDs pass through unchanged by default.** Anthropic `tool_use.id` == OpenAI `tool_call.id`. Do not regenerate. **Exception:** `openai::tool_normalization::normalize_request_tool_call_ids` with `ToolCallIdStrategy::NineDigitSequential` rewrites outbound IDs (and re-pairs the matching tool results) for providers that reject non-numeric/duplicate IDs — selected by `requires_safe_outbound_tool_ids` in the proxy (`mistral`/`codestral`/`openrouter`). `ToolCallIdStrategy::Preserve` (the default for everything else) keeps the pass-through invariant.
- **`arguments` (OpenAI, JSON string) vs `input` (Anthropic, JSON object).** The mapping layer serializes/deserializes across this boundary; don't assume same type.
- **`reasoning_content` <-> Anthropic thinking blocks** maps bidirectionally (DeepSeek/Qwen). Both directions must stay symmetric.
- **`anthropic::ErrorType::as_wire_str()` is the canonical snake_case stringifier.** Use it; never round-trip through `serde_json::to_value`. Adding a variant fails to compile until the `match` is handled.
- **Streaming uses a state machine with a bounded channel (32)** for backpressure. New stream types reuse the existing loop, don't fork it.
- **Golden fixtures** live in `fixtures/anthropic/` and `fixtures/openai/`. Add a fixture for any new mapping path.
- `ChatCompletionRequest` keeps unknown OpenAI fields via `#[serde(flatten)] extra: serde_json::Map`. Only add explicit struct fields when a field needs translation logic.
