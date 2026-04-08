# SambaNova

High-throughput inference on SN40L reconfigurable dataflow architecture, focused on large Llama and frontier models.

**LiteLLM prefix:** `sambanova/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.sambanova.ai/cloud/latest/get-started/overview.html

## Authentication

| Variable | Required | Description |
|---|---|---|
| `SAMBANOVA_API_KEY` | Yes | API key from cloud.sambanova.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=sambanova SAMBANOVA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=sambanova -e SAMBANOVA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: sambanova/Meta-Llama-3.3-70B-Instruct
      api_key: "env:SAMBANOVA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "Meta-Llama-3.3-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "Meta-Llama-3.3-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `Meta-Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B |
| `Meta-Llama-3.1-405B-Instruct` | 16k | Llama 3.1 405B |
| `Llama-4-Scout-17B-16E-Instruct` | 131k | Llama 4 Scout MoE |

## Notes

SambaNova's SN40L chip uses a dataflow architecture that avoids GPU memory bandwidth bottlenecks, enabling high throughput on large-parameter models. Tool use is not supported. Model IDs use the full HuggingFace-style name (e.g. `Meta-Llama-3.3-70B-Instruct`).
