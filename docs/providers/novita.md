# Novita AI

Serverless inference API with a broad model catalog including Llama, DeepSeek, and other open-weight models.

**LiteLLM prefix:** `novita/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://novita.ai/docs/api-reference/llm-api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NOVITA_API_KEY` | Yes | API key from novita.ai/settings |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=novita NOVITA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=novita -e NOVITA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.1-70b
    litellm_params:
      model: novita/meta-llama/llama-3.1-70b-instruct
      api_key: "env:NOVITA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/llama-3.1-70b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/llama-3.1-70b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `meta-llama/llama-3.1-70b-instruct` | 128k | Llama 3.1 70B |
| `deepseek/deepseek-v3` | 64k | DeepSeek V3 |

## Notes

Novita's API base is `https://api.novita.ai/v3/openai` (versioned path). Model IDs use lowercase `org/model` format. See https://novita.ai/model-api/llm for the full model list and current pricing.
