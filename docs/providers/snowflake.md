# Snowflake Cortex

Snowflake Cortex AI — managed LLM inference inside Snowflake, using JWT authentication against a per-account endpoint.

**LiteLLM prefix:** `snowflake/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.snowflake.com/en/user-guide/snowflake-cortex/llm-functions

## Authentication

| Variable | Required | Description |
|---|---|---|
| `SNOWFLAKE_JWT` | Yes | Snowflake key-pair JWT token |
| `SNOWFLAKE_ACCOUNT_ID` | Yes | Snowflake account identifier, e.g. `myorg-myaccount` |

Cortex AI uses key-pair JWT authentication, not password auth. Generate a JWT via the Snowflake CLI (`snow connection generate-jwt`) or the Snowflake Python connector. JWTs are short-lived; automate rotation if running in production.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=snowflake \
  SNOWFLAKE_JWT=your-jwt-token \
  OPENAI_BASE_URL=https://<account-id>.snowflakecomputing.com/api/v2/cortex/inference:complete \
  PROXY_OPEN_RELAY=true \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3-70b
    litellm_params:
      model: snowflake/llama3.3-70b
      api_key: "env:SNOWFLAKE_JWT"
      api_base: "https://<account-id>.snowflakecomputing.com/api/v2/cortex/inference:complete"
  - model_name: mistral-large
    litellm_params:
      model: snowflake/mistral-large2
      api_key: "env:SNOWFLAKE_JWT"
      api_base: "https://<account-id>.snowflakecomputing.com/api/v2/cortex/inference:complete"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama3.3-70b",
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
    "model": "llama3.3-70b",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
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

## Notable Models

| Model ID | Notes |
|---|---|
| `llama3.3-70b` | Meta Llama 3.3 70B |
| `mistral-large2` | Mistral Large 2 |
| `claude-3-5-sonnet` | Anthropic Claude 3.5 Sonnet (via Cortex) |

## Notes

- The endpoint URL is per-account: `https://<account-id>.snowflakecomputing.com/api/v2/cortex/inference:complete`. There is no global base URL.
- Authentication uses a JWT bearer token, not a static API key. JWTs expire (default 1 hour). For long-running proxy deployments, implement token refresh before expiry.
- Tool use, embeddings, and vision are not available through the Cortex inference endpoint.
- Model availability depends on your Snowflake region and edition. Check the Cortex documentation for per-region availability.
- Cortex AI is available on Business Critical and Enterprise editions. Standard edition access may be limited.
