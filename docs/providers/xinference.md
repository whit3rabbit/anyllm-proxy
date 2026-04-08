# Xinference

Self-hosted inference platform supporting a range of model types via an OpenAI-compatible API.

**LiteLLM prefix:** `xinference/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://inference.readthedocs.io/en/latest/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `XINFERENCE_SERVER_URL` | Yes | Base URL of the Xinference server (e.g. `http://localhost:9997/v1`) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=xinference XINFERENCE_SERVER_URL=http://localhost:9997/v1 PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run \
  -e BACKEND=xinference \
  -e OPENAI_BASE_URL=http://my-host:9997/v1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: xinference/qwen2-instruct
      api_base: "http://localhost:9997/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen2-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen2-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notes

Xinference has no fixed default URL. Set `XINFERENCE_SERVER_URL` (or `OPENAI_BASE_URL`) to point at your deployment.

Launch a model before sending requests:

```bash
xinference launch --model-name qwen2-instruct --model-format pytorch --size-in-billions 7
```

The model name in requests must match the `--model-name` used when launching. Run `xinference list --running` to see active models.

Xinference supports LLMs, embedding models, rerankers, and image models. Only the chat completions and embeddings paths are wired through this provider.
