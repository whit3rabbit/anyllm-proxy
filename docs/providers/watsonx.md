# IBM WatsonX

IBM WatsonX — enterprise AI platform offering foundation models including IBM Granite and hosted open models via an OpenAI-compatible endpoint.

**LiteLLM prefix:** `watsonx/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.ibm.com/docs/en/watsonx

## Authentication

| Variable | Required | Description |
|---|---|---|
| `WATSONX_API_KEY` | Yes | IBM Cloud API key |
| `WATSONX_URL` | Yes | WatsonX instance URL, e.g. `https://us-south.ml.cloud.ibm.com` |

Generate an IBM Cloud API key at https://cloud.ibm.com/iam/apikeys. The instance URL depends on your region — find it in the WatsonX project settings.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=watsonx \
  WATSONX_API_KEY=your-key \
  OPENAI_BASE_URL=https://us-south.ml.cloud.ibm.com/ml/v1/text/chat \
  PROXY_OPEN_RELAY=true \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: granite-chat
    litellm_params:
      model: watsonx/ibm/granite-13b-chat-v2
      api_key: "env:WATSONX_API_KEY"
      api_base: "https://us-south.ml.cloud.ibm.com/ml/v1/text/chat"
  - model_name: llama3-70b
    litellm_params:
      model: watsonx/meta-llama/llama-3-1-70b-instruct
      api_key: "env:WATSONX_API_KEY"
      api_base: "https://us-south.ml.cloud.ibm.com/ml/v1/text/chat"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ibm/granite-13b-chat-v2",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ibm/granite-13b-chat-v2",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
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

- The OpenAI-compatible endpoint on WatsonX is at `/ml/v1/text/chat` appended to your instance URL. Set `OPENAI_BASE_URL` or `api_base` to the full path including this suffix.
- Model IDs use a `provider/model-name` format (e.g., `ibm/granite-13b-chat-v2`, `meta-llama/llama-3-1-70b-instruct`). Pass the full ID as the `model` field in requests.
- Vision is not supported — WatsonX foundation models do not expose multimodal capabilities via this endpoint.
- No models are enumerated in the provider catalog. Available models depend on your WatsonX plan and region.
