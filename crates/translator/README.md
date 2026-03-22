# anthropic_openai_translate

Pure, IO-free translation between Anthropic Messages API and OpenAI Chat Completions / Responses API formats. Also supports Google Gemini native API translation.

No HTTP clients, no async runtime, no network calls. Just `fn(A) -> B` transformations.

## Quick Start

```rust
use anthropic_openai_translate::{TranslationConfig, translate_request, translate_response};
use anthropic_openai_translate::anthropic::MessageCreateRequest;

let config = TranslationConfig::builder()
    .model_map("haiku", "gpt-4o-mini")
    .model_map("sonnet", "gpt-4o")
    .model_map("opus", "gpt-4o")
    .build();

let req: MessageCreateRequest = serde_json::from_str(r#"{
    "model": "claude-sonnet-4-6",
    "max_tokens": 100,
    "messages": [{"role": "user", "content": "Hello"}]
}"#).unwrap();

let openai_req = translate_request(&req, &config).unwrap();
assert_eq!(openai_req.model, "gpt-4o");

// Send openai_req to OpenAI, get response, then:
// let anthropic_resp = translate_response(&openai_resp, &req.model);
```

## Supported APIs

- **Anthropic Messages API** (request/response types, streaming SSE events)
- **OpenAI Chat Completions API** (request/response types, streaming chunks)
- **OpenAI Responses API** (request/response types, streaming events)
- **Google Gemini native API** (generateContent types, streaming)

## Features

- **Default**: Pure translation types and mapping functions
- **`middleware`**: Adds an axum middleware layer that intercepts Anthropic-format requests, translates them, forwards to a configurable backend, and translates responses back

```toml
[dependencies]
anthropic_openai_translate = "0.1"

# With middleware support:
anthropic_openai_translate = { version = "0.1", features = ["middleware"] }
```

## Modules

| Module | Description |
|--------|-------------|
| `anthropic` | Anthropic Messages API types (request, response, streaming, errors) |
| `openai` | OpenAI Chat Completions and Responses API types |
| `gemini` | Google Gemini native API types |
| `mapping` | Stateless conversion functions between APIs |
| `config` | Translation configuration (model mapping, lossy behavior) |
| `translate` | Convenience wrappers combining config with mapping |
| `middleware` | Axum middleware layer (requires `middleware` feature) |

## Translation Coverage

- Text messages, multi-turn conversations
- System prompts (Anthropic `system` to OpenAI `developer` role or Responses `instructions`)
- Tool definitions, tool calls, tool results
- Image and document content blocks (documents degrade to text notes)
- Streaming SSE event translation (state machines for each backend)
- Token usage mapping
- Error type/status code translation
- Extended thinking (stripped when targeting OpenAI)

## Related

This crate is part of [llm-translate-api](https://github.com/whit3rabbit/llm-translate-api), which also includes a standalone HTTP proxy server.
