# Tool Calling Differences: Anthropic vs OpenAI

## Tool Definitions

| Aspect | Anthropic | OpenAI |
|---|---|---|
| Wrapper type | None (flat) | `{type: "function", function: {...}}` |
| Schema field | `input_schema` (JSON Schema) | `function.parameters` (JSON Schema) |
| Description | `description` (optional) | `function.description` (optional) |
| Name | `name` | `function.name` |

## Tool Use in Responses

| Aspect | Anthropic | OpenAI |
|---|---|---|
| Location | Content blocks in assistant message | `tool_calls[]` in assistant message |
| ID field | `tool_use.id` | `tool_call.id` |
| Name | `tool_use.name` | `tool_call.function.name` |
| Arguments | `tool_use.input` (JSON object) | `tool_call.function.arguments` (JSON string) |

## Tool Results

| Aspect | Anthropic | OpenAI |
|---|---|---|
| Location | Content block in user message | Separate `tool` role message |
| Reference | `tool_result.tool_use_id` | `tool_call_id` in message |
| Error flag | `is_error: true` | No standard error flag |
| Content | String or content blocks | String |

## Streaming Tool Calls

| Aspect | Anthropic | OpenAI |
|---|---|---|
| Start event | `content_block_start` with tool_use block | Chunk delta with `tool_calls[].id` |
| Argument deltas | `input_json_delta` (partial JSON) | `tool_calls[].function.arguments` (fragments) |
| End event | `content_block_stop` | `finish_reason: "tool_calls"` |

## ID Strategy

This proxy uses a stateless ID bridge: the OpenAI `tool_call.id` is used directly as the Anthropic `tool_use.id`. This avoids server-side session storage and allows the client to reference tool calls across turns without the proxy maintaining state.

## Known Limitations

- OpenAI `arguments` may contain invalid JSON (partial deltas, truncated output). The proxy uses lenient parsing that wraps unparseable strings as `Value::String`.
- Anthropic supports `is_error` on tool results; OpenAI has no equivalent. The proxy prepends "Error: " to error content.
- Parallel tool use control (`disable_parallel_tool_use` in Anthropic) has no direct OpenAI equivalent.
