# Docker Model Runner

Docker Desktop's built-in local model inference engine, powered by llama.cpp.

**LiteLLM prefix:** `docker_model_runner/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.docker.com/desktop/features/model-runner/

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=docker_model_runner PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=docker_model_runner -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

Override the default endpoint with `OPENAI_BASE_URL` if needed.

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: smollm2
    litellm_params:
      model: docker_model_runner/ai/smollm2
      api_base: "http://localhost:12434/engines/llama.cpp/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ai/smollm2", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ai/smollm2", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notes

Docker Model Runner is available in Docker Desktop 4.40 and later. Pull models before use:

```bash
docker model pull ai/smollm2
docker model pull ai/llama3.2
```

List available models with `docker model ls`. The inference endpoint is at `http://localhost:12434/engines/llama.cpp/v1`. Tool use and embeddings are not supported.
