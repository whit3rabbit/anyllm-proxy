# AI/ML API

Aggregator with 200+ models from multiple providers under a single OpenAI-compatible endpoint.

**LiteLLM prefix:** `aiml/`
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.aimlapi.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `AIML_API_KEY` | Yes | API key from aimlapi.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=aiml AIML_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=aiml -e AIML_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: aiml/gpt-4o
      api_key: "env:AIML_API_KEY"
  - model_name: llama-3.3-70b
    litellm_params:
      model: aiml/meta-llama/Llama-3.3-70B-Instruct
      api_key: "env:AIML_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.3-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.3-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `gpt-4o` | 128k | OpenAI GPT-4o via AIML aggregation |
| `claude-3-5-sonnet` | 200k | Anthropic Claude 3.5 Sonnet via AIML aggregation |
| `meta-llama/Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B instruction-tuned |

## Notes

AI/ML API aggregates models from OpenAI, Anthropic, Meta, Mistral, and others. Model IDs vary by provider: OpenAI models use bare names (`gpt-4o`), open-source models use `org/name` format. Check https://docs.aimlapi.com/api-overview/models-gallery for the full model list.
