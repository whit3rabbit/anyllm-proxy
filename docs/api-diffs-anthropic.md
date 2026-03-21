# Anthropic API: Type Verification Notes

## Verified Against

Anthropic Messages API documentation as referenced in PLAN.md.

## MessageCreateRequest

| Field | Type | Required | Implemented | Notes |
|---|---|---|---|---|
| model | string | Yes | Yes | |
| max_tokens | u32 | Yes | Yes | |
| messages | Vec<InputMessage> | Yes | Yes | |
| system | Option<System> | No | Yes | String or blocks |
| temperature | Option<f32> | No | Yes | Range 0..1 |
| top_p | Option<f32> | No | Yes | |
| stop_sequences | Option<Vec<String>> | No | Yes | |
| tools | Option<Vec<Tool>> | No | Yes | |
| tool_choice | Option<ToolChoice> | No | Yes | auto/any/none/tool |
| metadata | Option<Metadata> | No | Yes | user_id only |
| stream | Option<bool> | No | Yes | |
| thinking | - | No | Not implemented | Extended thinking config |

## Content Blocks

| Block Type | Implemented | Notes |
|---|---|---|
| text | Yes | |
| image | Yes | base64 and URL sources |
| document | Yes | Converted to text note (no inline PDF in Chat Completions) |
| tool_use | Yes | id, name, input (JSON object) |
| tool_result | Yes | tool_use_id, content (string or blocks), is_error |
| thinking | No | Extended thinking not proxied |

## MessageResponse

| Field | Implemented | Notes |
|---|---|---|
| id | Yes | Generated as msg_{uuid} |
| type | Yes | Always "message" |
| role | Yes | Always "assistant" |
| content | Yes | Vec<ContentBlock> |
| model | Yes | Echoed from request |
| stop_reason | Yes | end_turn, max_tokens, stop_sequence, tool_use |
| stop_sequence | Yes | Always null (not tracked from OpenAI) |
| usage | Yes | input_tokens, output_tokens |

## Error Types

All 8 error types implemented: invalid_request_error, authentication_error, permission_error, not_found_error, request_too_large, rate_limit_error, api_error, overloaded_error.

## Streaming Events

All event types implemented: message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop, ping, error.

Delta types: text_delta, input_json_delta. Thinking_delta not implemented.

## Known Gaps

- Extended thinking (`thinking` config and `thinking_delta` events)
- `pause_turn` stop reason (no OpenAI equivalent)
- Prompt caching (`cache_control` on system blocks parsed but not utilized)
- Beta Files API
- Beta Skills API
