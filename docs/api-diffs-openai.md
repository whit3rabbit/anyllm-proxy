# OpenAI API: Type Verification Notes

## Verified Against

OpenAI Chat Completions and Responses API documentation as referenced in PLAN.md.

## ChatCompletionRequest

| Field | Type | Implemented | Notes |
|---|---|---|---|
| model | string | Yes | |
| messages | Vec<ChatMessage> | Yes | |
| max_tokens | Option<u32> | Yes | |
| temperature | Option<f32> | Yes | Range 0..2 |
| top_p | Option<f32> | Yes | |
| stop | Option<Stop> | Yes | Single string or array (max 4) |
| tools | Option<Vec<ChatTool>> | Yes | Function tools |
| tool_choice | Option<ChatToolChoice> | Yes | auto/none/required/named |
| stream | Option<bool> | Yes | |
| stream_options | Option<StreamOptions> | Yes | include_usage |
| presence_penalty | Option<f32> | Yes (field exists) | Not populated from Anthropic |
| frequency_penalty | Option<f32> | Yes (field exists) | Not populated from Anthropic |
| response_format | Option<ResponseFormat> | Yes (field exists) | Not populated from Anthropic |

## ChatMessage Roles

| Role | Implemented | Notes |
|---|---|---|
| system | Yes | Legacy, still supported |
| developer | Yes | Used for Anthropic system prompt |
| user | Yes | |
| assistant | Yes | |
| tool | Yes | For tool results |

## ChatCompletionResponse

| Field | Implemented | Notes |
|---|---|---|
| id | Yes | |
| object | Yes | "chat.completion" |
| model | Yes | |
| choices | Yes | Vec<Choice> |
| usage | Yes | Optional |
| created | Yes | Optional |
| system_fingerprint | Yes | Optional |

## Finish Reasons

| Reason | Implemented | Anthropic Mapping |
|---|---|---|
| stop | Yes | end_turn |
| length | Yes | max_tokens |
| tool_calls | Yes | tool_use |
| content_filter | Yes | end_turn (approximate) |
| function_call | Yes | tool_use (deprecated) |

## Streaming Chunks

ChatCompletionChunk fully implemented with ChunkChoice, ChunkDelta, ChunkToolCall, ChunkFunctionCall.

## Responses API

Basic types implemented (ResponsesRequest, ResponsesResponse, ResponsesUsage) for potential future use. Not actively used in translation since we target Chat Completions.

## Error Types

ErrorResponse with ErrorDetail (message, type, param, code) implemented.

## Known Gaps

- Responses API not used as backend target (Chat Completions only)
- WebSocket mode not supported
- Structured outputs (response_format) not translated from Anthropic
- MCP tools not supported
- Realtime API not supported
