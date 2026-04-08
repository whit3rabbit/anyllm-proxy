# Ollama

Run open-weight models locally via Ollama's OpenAI-compatible API.

**LiteLLM prefix:** `ollama/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://ollama.com/blog/openai-compatibility

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=ollama PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run -e BACKEND=ollama -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

Override the default endpoint with `OPENAI_BASE_URL` if Ollama is not on localhost.

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: ollama/llama3.2
      api_base: "http://localhost:11434/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2", "messages": [{"role": "user", "content": "Hello"}]}'
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

Models must be pulled before use:

```bash
ollama pull llama3.2
ollama pull nomic-embed-text   # for embeddings
```

The default endpoint is `http://localhost:11434/v1`. Set `OPENAI_BASE_URL` to point at a remote Ollama instance.

Available models depend entirely on what has been pulled locally. Run `ollama list` to see what is installed.
