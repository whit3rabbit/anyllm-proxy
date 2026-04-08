# LM Studio

Desktop application for running GGUF models locally, with a built-in OpenAI-compatible server.

**LiteLLM prefix:** `lm_studio/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://lmstudio.ai/docs/local-server

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=lm_studio PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run -e BACKEND=lm_studio -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: lm_studio/lmstudio-community/Meta-Llama-3.1-8B-Instruct-GGUF
      api_base: "http://localhost:1234/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "lmstudio-community/Meta-Llama-3.1-8B-Instruct-GGUF", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "lmstudio-community/Meta-Llama-3.1-8B-Instruct-GGUF", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notes

Start the local server from within the LM Studio application: open the Local Server tab and click Start Server. The default address is `http://localhost:1234/v1`.

LM Studio loads GGUF-format models. Download models from the Discover tab inside the app, or place `.gguf` files in the LM Studio models directory manually.

The model name in requests corresponds to the model identifier shown in LM Studio's model selector.
