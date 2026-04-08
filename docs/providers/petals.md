# Petals

Distributed inference framework that runs large models collaboratively across multiple machines.

**LiteLLM prefix:** `petals/`
**Status:** Stub — routes through OpenAI-compatible client
**Docs:** https://github.com/bigscience-workshop/petals

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=petals PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# or Docker:
docker run -e BACKEND=petals -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: local-model
    litellm_params:
      model: petals/petals-team/StableBeluga2
      api_base: "http://localhost:8080"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "petals-team/StableBeluga2", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "petals-team/StableBeluga2", "messages": [{"role": "user", "content": "Hello"}]}'
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

Petals splits model layers across participating machines; each node contributes GPU memory to run models too large for a single device.

Install the Petals server and start it:

```bash
pip install petals
python -m petals.cli.run_server petals-team/StableBeluga2
```

The OpenAI-compatible HTTP endpoint listens on port 8080 by default. The model name in requests must match the Hugging Face model ID being served.

Petals is best suited for research and experimentation, not latency-sensitive production workloads. Throughput depends on network bandwidth between nodes.
