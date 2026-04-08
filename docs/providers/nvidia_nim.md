# NVIDIA NIM

NVIDIA's hosted inference microservices, providing access to a wide range of models including Llama, Nemotron, and others via an OpenAI-compatible API.

**LiteLLM prefix:** `nvidia_nim/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.api.nvidia.com/nim/reference

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NVIDIA_NIM_API_KEY` | Yes | API key obtained from build.nvidia.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=nvidia_nim NVIDIA_NIM_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=nvidia_nim -e NVIDIA_NIM_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-405b
    litellm_params:
      model: nvidia_nim/meta/llama-3.1-405b-instruct
      api_key: "env:NVIDIA_NIM_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta/llama-3.1-405b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta/llama-3.1-405b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `meta/llama-3.1-405b-instruct` | 128k | Meta Llama 3.1 405B |
| `nvidia/llama-3.1-nemotron-70b-instruct` | 128k | NVIDIA fine-tune for instruction following |

## Notes

- API keys are issued at build.nvidia.com.
- Model IDs use a `org/model-name` format (e.g. `meta/llama-3.1-405b-instruct`).
- NIM containers can also be self-hosted on NVIDIA GPU infrastructure. Point `NVIDIA_NIM_BASE_URL` at your local endpoint.
- The hosted API endpoint is `https://integrate.api.nvidia.com/v1`.
