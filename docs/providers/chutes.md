# Chutes AI

Serverless inference platform for open-source models with pay-per-token pricing.

**LiteLLM prefix:** `chutes/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://chutes.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `CHUTES_API_KEY` | Yes | API key from chutes.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=chutes CHUTES_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=chutes -e CHUTES_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: deepseek-v3
    litellm_params:
      model: chutes/deepseek-ai/DeepSeek-V3
      api_key: "env:CHUTES_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "deepseek-ai/DeepSeek-V3", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "deepseek-ai/DeepSeek-V3", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `deepseek-ai/DeepSeek-V3` | 128k | DeepSeek V3 |
| `meta-llama/Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B instruction-tuned |

## Notes

Model IDs use `org/model-name` format matching HuggingFace naming conventions. Check https://chutes.ai for the current model catalog, as availability changes with demand.
