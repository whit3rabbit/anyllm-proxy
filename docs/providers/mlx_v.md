# mlx-v

Rust vision-language-model inference toolkit on the candle backend, with a built-in OpenAI-compatible server (`vlm serve`).

**LiteLLM prefix:** `mlx_v/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://github.com/whit3rabbit/mlx-v

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | Chat completions are open. `vlm serve --api-key` gates only `/metrics`, `/cache/*`, and `/unload` |

## Quick Start

Start the model server first. It serves one model per process:

```bash
vlm serve --model models/Qwen2-VL-2B-Instruct --host 127.0.0.1 --port 8080
```

### Single-Backend (env vars)

```bash
BACKEND=mlx_v PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run -e BACKEND=mlx_v -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-vlm
    litellm_params:
      model: mlx_v/models/Qwen2-VL-2B-Instruct
      api_base: "http://localhost:8080/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "models/Qwen2-VL-2B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "models/Qwen2-VL-2B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |

## Notes

One model per process. The model name in a request is the `--model` string as typed. To serve a second model, start a second `vlm serve` on another port and add it as another backend.

`--host` defaults to `0.0.0.0`, so bind `127.0.0.1` explicitly when the proxy is on the same machine. Chat completions are not behind `--api-key`; put the proxy in front if you need real auth on them.

Images must be inline `data:` URLs. mlx-v refuses filesystem paths and `http(s)` URLs in a request body on purpose: it would be arbitrary-file-read and request forgery on an open port. Inline the bytes before forwarding.

Tool calling honors `tool_choice` values `"auto"` and `"none"` only. `"required"` and a named function are rejected with a 400, because mlx-v has no constrained decoding to keep that promise with. Tool-call parsing covers the Hermes/Qwen `<tool_call>` output format, so pair it with a Qwen-family checkpoint.

There is no `/v1/embeddings` route. Route embeddings to a different backend.
