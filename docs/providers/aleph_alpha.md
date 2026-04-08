# Aleph Alpha

European sovereign AI provider offering the Luminous model family with a focus on data privacy and compliance.

**LiteLLM prefix:** `aleph_alpha/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.aleph-alpha.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `ALEPH_ALPHA_API_KEY` | Yes | API key from app.aleph-alpha.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=aleph_alpha ALEPH_ALPHA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=aleph_alpha -e ALEPH_ALPHA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: luminous-supreme
    litellm_params:
      model: aleph_alpha/luminous-supreme-control
      api_key: "env:ALEPH_ALPHA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "luminous-supreme-control", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "luminous-supreme-control", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `luminous-supreme-control` | 2k | Highest capability, instruction-tuned |
| `luminous-extended` | 2k | Mid-tier, general purpose |
| `luminous-base` | 2k | Smallest, fastest |

## Notes

Aleph Alpha is headquartered in Germany. Data processed via their API stays within EU infrastructure, which may be relevant for GDPR and EU AI Act compliance. Tool use is not supported through the OpenAI-compatible interface. For embedding workloads, the Luminous models produce semantic vectors suitable for retrieval tasks.
