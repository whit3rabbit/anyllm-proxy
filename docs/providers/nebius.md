# Nebius AI Studio

EU-sovereign GPU cloud offering serverless inference on open-weight models, operated by Nebius (formerly Yandex Cloud).

**LiteLLM prefix:** `nebius/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://studio.nebius.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NEBIUS_API_KEY` | Yes | API key from studio.nebius.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=nebius NEBIUS_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=nebius -e NEBIUS_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.1-70b
    litellm_params:
      model: nebius/meta-llama/Meta-Llama-3.1-70B-Instruct
      api_key: "env:NEBIUS_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `meta-llama/Meta-Llama-3.1-70B-Instruct` | 128k | Llama 3.1 70B |
| `Qwen/Qwen2.5-72B-Instruct` | 128k | Qwen 2.5 72B |

## Notes

Nebius operates data centers in the EU (Finland), making it a viable option for workloads with EU data residency requirements. Model IDs use the full HuggingFace-style `org/model` format. The embedding endpoint uses the same OpenAI-compatible API base.
