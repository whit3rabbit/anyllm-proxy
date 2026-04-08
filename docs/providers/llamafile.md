# llamafile

Single-file executable that bundles a model and a local inference server.

**LiteLLM prefix:** `llamafile/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://github.com/Mozilla-Ocho/llamafile

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=llamafile PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run -e BACKEND=llamafile -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: llamafile/local-model
      api_base: "http://localhost:8080"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "local-model", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "local-model", "messages": [{"role": "user", "content": "Hello"}]}'
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

Download the `.llamafile` for the model you want, make it executable, and run it:

```bash
wget https://huggingface.co/Mozilla/Meta-Llama-3.1-8B-Instruct-llamafile/resolve/main/Meta-Llama-3.1-8B-Instruct.Q6_K.llamafile
chmod +x Meta-Llama-3.1-8B-Instruct.Q6_K.llamafile
./Meta-Llama-3.1-8B-Instruct.Q6_K.llamafile --server --port 8080
```

The server starts on port 8080 by default and exposes an OpenAI-compatible `/v1/chat/completions` endpoint. The model name used in requests is ignored by llamafile; any non-empty string works.

Tool use and embeddings are not supported by the llamafile server implementation.
