# Compatibility Contract

## Supported Features

| Feature | Status | Notes |
|---|---|---|
| Basic text messages | Supported | Full fidelity |
| Multi-turn conversations | Supported | Role mapping handled |
| System prompts (string) | Supported | Mapped to developer role |
| System prompts (blocks) | Supported | Concatenated to developer message |
| Streaming (SSE) | Supported | Full event sequence translation |
| Tool definitions | Supported | input_schema -> function.parameters |
| Tool use responses | Supported | Stateless ID bridge |
| Tool result handling | Supported | tool_result -> tool role message |
| Multiple tool calls | Supported | Parallel calls preserved |
| Image content (base64) | Supported | Converted to data URI |
| Image content (URL) | Supported | Direct pass-through |
| Stop sequences | Supported | Capped at 4 (OpenAI limit) |
| Temperature | Supported | Pass-through (0..1 subset of 0..2) |
| top_p | Supported | Direct pass-through |
| GET /v1/models | Supported | Static model list |

## Unsupported Features (Explicit Error)

| Feature | Error | Notes |
|---|---|---|
| Token counting | 400 invalid_request_error | No local approximation |
| Batch processing | 400 invalid_request_error | Would require Batch API mapping |

## Approximated Features (Best Effort)

| Feature | Approximation | Risk |
|---|---|---|
| Documents (PDF) | Text note placeholder | Content not processed by model |
| content_filter finish | Mapped to end_turn | Semantic difference |
| Token accounting | prompt_tokens = input_tokens | Not exact due to tokenizer differences |
| cache_* usage fields | Always null | No prompt caching emulation |

## Not Implemented

| Feature | Reason |
|---|---|
| Extended thinking | No OpenAI equivalent |
| pause_turn stop reason | Anthropic-specific |
| Prompt caching | Different mechanisms |
| Beta Files API | Different semantics |
| Beta Skills API | Different packaging |
| MCP tools | Would need Responses API |
| WebSocket mode | Anthropic doesn't support |

## Model Name Mapping

Model names are passed through as-is. The static /v1/models endpoint lists:
- claude-opus-4-6
- claude-sonnet-4-6
- claude-haiku-4-5-20251001

Clients may use any model name; it's forwarded to OpenAI directly. Configure model aliasing at the OpenAI provider level if needed.

## Endpoint Mapping

| Anthropic Endpoint | Proxy Route | Backend |
|---|---|---|
| POST /v1/messages | POST /v1/messages | POST /v1/chat/completions |
| GET /v1/models | GET /v1/models | Static response |
| POST /v1/messages/count_tokens | POST /v1/messages/count_tokens | 400 error |
| POST /v1/messages/batches | POST /v1/messages/batches | 400 error |
| GET /health | GET /health | Local (no auth required) |
