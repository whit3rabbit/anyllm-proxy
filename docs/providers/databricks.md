# Databricks

Model serving endpoints hosted within a Databricks workspace, supporting both Databricks foundation models and custom-deployed models.

**LiteLLM prefix:** `databricks/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.databricks.com/en/machine-learning/foundation-models/api-reference.html

## Authentication

| Variable | Required | Description |
|---|---|---|
| `DATABRICKS_API_KEY` | Yes | Personal access token or service principal token; also accepted as `DATABRICKS_TOKEN` |
| `OPENAI_BASE_URL` | Yes | Your workspace serving endpoint, e.g. `https://adb-<id>.azuredatabricks.net/serving-endpoints` |

The workspace URL is required because there is no shared Databricks endpoint — every workspace has its own URL. Set `OPENAI_BASE_URL` to override the (empty) default base URL, or use `api_base` in the LiteLLM YAML config.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=databricks \
  DATABRICKS_API_KEY=your-token \
  OPENAI_BASE_URL=https://adb-1234567890.azuredatabricks.net/serving-endpoints \
  cargo run -p anyllm_proxy
# Docker:
docker run \
  -e BACKEND=databricks \
  -e DATABRICKS_API_KEY=your-token \
  -e OPENAI_BASE_URL=https://adb-1234567890.azuredatabricks.net/serving-endpoints \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: dbrx
    litellm_params:
      model: databricks/databricks-dbrx-instruct
      api_key: "env:DATABRICKS_API_KEY"
      api_base: "https://adb-1234567890.azuredatabricks.net/serving-endpoints"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "databricks-dbrx-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "databricks-dbrx-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `databricks-dbrx-instruct` | 32k | DBRX, Databricks' MoE model |
| `databricks-meta-llama-3-3-70b-instruct` | 128k | Managed Llama 3.3 70B |

## Notes

Each Databricks workspace exposes its own serving endpoint URL. There is no single shared base URL. When routing multiple models from different workspaces, use per-model `api_base` in the LiteLLM YAML config rather than the global `OPENAI_BASE_URL`. Custom-deployed models also appear under the same endpoint and follow the same API contract.
