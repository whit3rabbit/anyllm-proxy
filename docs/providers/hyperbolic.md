# Hyperbolic

Affordable GPU inference platform supporting large open-source models including vision-capable variants.

**LiteLLM prefix:** `hyperbolic/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.hyperbolic.xyz

## Authentication

| Variable | Required | Description |
|---|---|---|
| `HYPERBOLIC_API_KEY` | Yes | API key from app.hyperbolic.xyz |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=hyperbolic HYPERBOLIC_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=hyperbolic -e HYPERBOLIC_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: qwen2.5-72b
    litellm_params:
      model: hyperbolic/Qwen/Qwen2.5-72B-Instruct
      api_key: "env:HYPERBOLIC_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "Qwen/Qwen2.5-72B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "Qwen/Qwen2.5-72B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `Qwen/Qwen2.5-72B-Instruct` | 128k | Strong general-purpose model |
| `meta-llama/Meta-Llama-3.1-405B-Instruct` | 128k | Largest Llama 3.1 |
| `deepseek-ai/DeepSeek-V3` | 128k | DeepSeek V3 |
| `meta-llama/Llama-3.2-90B-Vision-Instruct` | 128k | Vision-capable Llama 3.2 |

## Notes

Model IDs use the HuggingFace `org/model-name` format. Vision support is model-dependent; not all models listed accept image inputs. Check https://app.hyperbolic.xyz/models for the current catalog and pricing.
