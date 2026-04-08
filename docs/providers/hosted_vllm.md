# vLLM (self-hosted)

Production-grade self-hosted inference server with an OpenAI-compatible API.

**LiteLLM prefix:** `hosted_vllm/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://docs.vllm.ai/en/latest/serving/openai_compatible_server.html

## Authentication

| Variable | Required | Description |
|---|---|---|
| `VLLM_API_KEY` | No | Bearer token, required only if vllm was started with `--api-key` |
| `OPENAI_BASE_URL` | Yes | URL of the vLLM server (e.g. `http://my-host:8000/v1`) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=hosted_vllm OPENAI_BASE_URL=http://my-host:8000/v1 VLLM_API_KEY=secret PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run \
  -e BACKEND=hosted_vllm \
  -e OPENAI_BASE_URL=http://my-host:8000/v1 \
  -e VLLM_API_KEY=secret \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: hosted_vllm/meta-llama/Meta-Llama-3.1-8B-Instruct
      api_base: "http://my-host:8000/v1"
      api_key: "secret"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notes

vLLM has no fixed default URL; `OPENAI_BASE_URL` is required. Each deployment has its own host and port.

Authentication is optional. The `--api-key` flag on `vllm serve` enables it:

```bash
vllm serve meta-llama/Meta-Llama-3.1-8B-Instruct --api-key secret
```

If authentication is not configured on the vLLM server, omit `VLLM_API_KEY`.

The model name in requests must match the model name passed to `vllm serve`.
