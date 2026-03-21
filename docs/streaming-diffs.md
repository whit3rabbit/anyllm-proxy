# Streaming Protocol Differences

## Anthropic SSE Event Sequence

```
event: message_start
data: {"type":"message_start","message":{...initial message with empty content...}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}

event: message_stop
data: {"type":"message_stop"}
```

## OpenAI Chat Completions Streaming

```
data: {"id":"chatcmpl-...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}

data: [DONE]
```

## Translation State Machine

The `StreamingTranslator` converts OpenAI chunks to Anthropic events:

1. **First chunk** -> emit `message_start` with empty content
2. **Text delta** -> emit `content_block_start` (if not open) + `content_block_delta` with `text_delta`
3. **Tool call start** (has `id`) -> close any open text block, emit `content_block_start` with `tool_use`
4. **Tool call args** -> emit `content_block_delta` with `input_json_delta`
5. **finish_reason** -> close open blocks, emit `message_delta` with mapped stop_reason
6. **[DONE]** -> emit `message_stop`

## Backpressure

- Bounded mpsc channel (capacity 32) between upstream reader and SSE emitter
- Client disconnect detected via `tx.send().is_err()`
- Upstream connection dropped when receiver is dropped
