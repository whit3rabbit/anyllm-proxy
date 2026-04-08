# NLP Cloud

Hosted NLP inference API with a focus on fine-tuned and conversational models.

**LiteLLM prefix:** `nlp_cloud/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.nlpcloud.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NLP_CLOUD_API_KEY` | Yes | API key from nlpcloud.com/home/token |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=nlp_cloud NLP_CLOUD_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=nlp_cloud -e NLP_CLOUD_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: finetuned-llama-3-70b
    litellm_params:
      model: nlp_cloud/finetuned-llama-3-70b
      api_key: "env:NLP_CLOUD_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "finetuned-llama-3-70b", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "finetuned-llama-3-70b", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `finetuned-llama-3-70b` | 8k | Fine-tuned Llama 3 70B |
| `dolphin` | 4k | Dolphin conversational model |
| `chatdolphin` | 4k | Chat-optimized Dolphin variant |

## Notes

NLP Cloud provides GPU-accelerated hosting primarily for fine-tuned open-source models. Tool use is not supported through the OpenAI-compatible interface. Context window sizes vary by model; check https://docs.nlpcloud.com/#models for the full model list and current limits.
