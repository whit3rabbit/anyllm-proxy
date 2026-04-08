# Lemonade

Local LLM inference server optimized for AMD ROCm GPUs.

**LiteLLM prefix:** `lemonade/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://github.com/lemonade-sdk/lemonade

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=lemonade PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=lemonade -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

Override the default endpoint with `OPENAI_BASE_URL` if Lemonade is not on localhost.

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: lemonade/local-model
      api_base: "http://localhost:8000"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "local-model", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "local-model", "messages": [{"role": "user", "content": "Hello"}]}'
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

Lemonade targets AMD ROCm GPU hardware for local inference. The server listens on port 8000 by default. Install and start Lemonade before running the proxy; refer to the Lemonade documentation for model loading instructions. Tool use and embeddings are not supported.
