# NVIDIA Triton

NVIDIA Triton Inference Server with an OpenAI-compatible frontend via the TensorRT-LLM backend.

**LiteLLM prefix:** `triton/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://github.com/triton-inference-server/tensorrtllm_backend

## Authentication

| Variable | Required | Description |
|---|---|---|
| `OPENAI_BASE_URL` | Yes | URL of the Triton OpenAI-compatible endpoint (e.g. `http://my-host:8000/v1`) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=triton OPENAI_BASE_URL=http://my-host:8000/v1 PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run \
  -e BACKEND=triton \
  -e OPENAI_BASE_URL=http://my-host:8000/v1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: triton/ensemble
      api_base: "http://my-host:8000/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ensemble", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ensemble", "messages": [{"role": "user", "content": "Hello"}]}'
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

Triton's native protocol is gRPC/HTTP but does not expose an OpenAI-compatible API by default. The OpenAI-compatible frontend requires the TensorRT-LLM backend (`tensorrtllm_backend`) and its bundled API server.

Triton has no fixed default URL. Set `OPENAI_BASE_URL` to the address of your deployment.

The model name in requests corresponds to the Triton model repository name (commonly `ensemble` in TRT-LLM deployments). Check your model repository for the correct name.

Triton is production-grade but requires significant setup: GPU drivers, TensorRT-LLM engine compilation, and a configured model repository. Not suitable for quick local experimentation; use Ollama or LM Studio for that.
