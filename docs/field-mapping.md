# Field Mapping: Anthropic -> OpenAI Chat Completions

## Request Fields

| Anthropic Field | OpenAI Field | Mapping Rule | Notes |
|---|---|---|---|
| `model` | `model` | Pass through | Model name kept as-is; aliasing is config-level |
| `max_tokens` (required) | `max_tokens` | Direct copy | |
| `system` (string) | `messages[0]` role=developer | Wrap as developer message | Prefer `developer` over `system` for newer models |
| `system` (blocks) | `messages[0]` role=developer | Concatenate block texts with newline | |
| `messages[].role=user` | `messages[].role=user` | Direct | Content format differs (see below) |
| `messages[].role=assistant` | `messages[].role=assistant` | Direct | Tool calls extracted separately |
| `temperature` (0..1) | `temperature` (0..2) | Pass through | Anthropic range is subset of OpenAI |
| `top_p` | `top_p` | Pass through | Same semantics |
| `stop_sequences[]` | `stop` (string[]) | Take first 4 | OpenAI caps at 4; silent truncation |
| `stream` | `stream` | Pass through | SSE format translation required |
| `tools[].name` | `tools[].function.name` | Direct | |
| `tools[].description` | `tools[].function.description` | Direct | |
| `tools[].input_schema` | `tools[].function.parameters` | Direct (JSON Schema) | |
| `tool_choice.type=auto` | `tool_choice="auto"` | Simple string | |
| `tool_choice.type=any` | `tool_choice="required"` | Semantic equivalent | |
| `tool_choice.type=none` | `tool_choice="none"` | Direct | |
| `tool_choice.type=tool` | `tool_choice={type:"function",function:{name}}` | Named choice | |
| `metadata.user_id` | Not mapped | Dropped | No direct equivalent |

## Content Block Mapping (User Messages)

| Anthropic Block | OpenAI Equivalent | Notes |
|---|---|---|
| `text` | `text` content part | Direct |
| `image` (base64) | `image_url` with data URI | `data:{media_type};base64,{data}` |
| `image` (url) | `image_url` with URL | Direct |
| `document` (base64) | Text note (fallback) | Chat Completions lacks inline PDF support |
| `tool_result` | `tool` role message | Separate message with `tool_call_id` |

## Content Block Mapping (Assistant Messages)

| Anthropic Block | OpenAI Equivalent | Notes |
|---|---|---|
| `text` | `content` string | Concatenated if multiple |
| `tool_use` | `tool_calls[]` entry | `input` (object) -> `arguments` (JSON string) |

## Response Fields

| OpenAI Field | Anthropic Field | Mapping Rule |
|---|---|---|
| `choices[0].message.content` | `content[].type=text` | Wrap as text block |
| `choices[0].message.tool_calls` | `content[].type=tool_use` | `arguments` (string) -> `input` (object) |
| `choices[0].finish_reason=stop` | `stop_reason=end_turn` | |
| `choices[0].finish_reason=length` | `stop_reason=max_tokens` | |
| `choices[0].finish_reason=tool_calls` | `stop_reason=tool_use` | |
| `choices[0].finish_reason=content_filter` | `stop_reason=end_turn` | Approximate |
| `usage.prompt_tokens` | `usage.input_tokens` | Direct |
| `usage.completion_tokens` | `usage.output_tokens` | Direct |
| `usage.total_tokens` | Not mapped | Computed, not stored |
