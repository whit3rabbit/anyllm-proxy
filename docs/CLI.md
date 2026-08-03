# Command Line Interface (CLI) & Advanced Configuration

This document covers running `anyllm-proxy` directly from the command line, configuring backends via environment variables/config files, and interacting with the admin API using `curl`.

---

## Command Line Usage

### Standard Server Command
Start the proxy server and the admin WebUI:
```bash
anyllm-proxy --webui
```

### Running a Tool Wrapper
The proxy can launch a CLI tool (like `claude`) and configure it to point to the proxy automatically in one command:
```bash
anyllm-proxy run claude
```
This is equivalent to manually setting `ANTHROPIC_BASE_URL` and running `claude`.

### Using Specific Env Files
Load settings from a specific `.env` file instead of the default `.anyllm.env`:
```bash
anyllm-proxy --env-file ~/configs/ollama.env
```

---

## Environment Variables Configuration

If you do not want to use the WebUI settings dashboard, you can configure backends and models using environment variables.

### Local LLMs (Ollama, LM Studio, vLLM)
```bash
OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:11434/v1 \
BIG_MODEL=qwen2.5-coder:32b \
SMALL_MODEL=qwen2.5-coder:32b \
anyllm-proxy
```
For **LM Studio** use port `1234` and for **vLLM** use port `8000` (substitute `OPENAI_BASE_URL`).
If your local LLM rejects `stream_options`, set `OMIT_STREAM_OPTIONS=true`.

### Commercial APIs

#### OpenAI
```bash
OPENAI_API_KEY=sk-... \
BIG_MODEL=gpt-4o \
SMALL_MODEL=gpt-4o-mini \
anyllm-proxy
```

#### OpenRouter
```bash
# Using the dedicated provider key (recommended):
BACKEND=openrouter \
OPENROUTER_API_KEY=sk-or-... \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
anyllm-proxy

# Or via the generic OpenAI-compat path:
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
anyllm-proxy
```

#### Google Gemini
```bash
BACKEND=gemini \
GEMINI_API_KEY=AIza... \
anyllm-proxy
```

#### Azure OpenAI
```bash
BACKEND=azure \
AZURE_OPENAI_ENDPOINT=https://myresource.openai.azure.com \
AZURE_OPENAI_DEPLOYMENT=my-gpt4o \
AZURE_OPENAI_API_KEY=... \
anyllm-proxy
```

#### AWS Bedrock
```bash
BACKEND=bedrock \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
BIG_MODEL=anthropic.claude-3-5-sonnet-20241022-v2:0 \
SMALL_MODEL=anthropic.claude-3-5-haiku-20241022-v1:0 \
anyllm-proxy
```

#### Anthropic Passthrough
(No translation: forwards requests directly to Anthropic, useful for billing, metrics, or rate-limiting control):
```bash
BACKEND=anthropic \
ANTHROPIC_API_KEY=sk-ant-... \
anyllm-proxy
```

### Advanced Inbound Subscription/Credential Sharing
When using Claude Code with a subscription (no API key needed), the proxy must have the upstream credentials to talk to Anthropic.

#### Option A: Portable token
```bash
# 1. On a machine logged into Claude Code, mint a bearer token:
claude setup-token

# 2. Start proxy with it:
BACKEND=anthropic ANTHROPIC_AUTH_TOKEN=<token-from-setup-token> \
PROXY_OPEN_RELAY=true \
anyllm-proxy

# 3. Run Claude Code pointing to the proxy:
ANTHROPIC_BASE_URL=http://localhost:3000 ANTHROPIC_API_KEY=proxy-user claude
```

#### Option B: Forward client authentication
Set `ANTHROPIC_FORWARD_CLIENT_AUTH=true` to forward whatever credentials Claude Code passes, bypassing the token minting:
```bash
BACKEND=anthropic ANTHROPIC_FORWARD_CLIENT_AUTH=true \
PROXY_OPEN_RELAY=true \
anyllm-proxy
```
For security safeguards on client auth forwarding, see [ENV.md](ENV.md).

---

## Multi-Backend Routing & Configuration Files

You can define multiple backends using TOML or LiteLLM YAML config files. Point the proxy to the config file via `PROXY_CONFIG`.

### Auto Router & Claude Code Model Discovery

The admin UI's **Auto Router** tab maps Claude Code request tiers (Default, Background, Think, Long Context, Web Search, Image) to a specific backend and model. When the router is enabled, `GET /v1/models` advertises the real backend models (the tier targets plus each managed backend's catalog) instead of a static Claude catalog, so Claude Code can show and pick the actual models.

The tab's **Start Claude Code** command includes `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=true`, which makes Claude Code fetch `/v1/models` and add those real models to its `/model` picker. A model picked from that list is routed straight to the backend that offers it (explicit pick wins, bypassing tier-signal routing); `claude-*` alias traffic still flows through the configured tiers as before.

### TOML Format
```toml
# config.toml
listen_port = 3000
default_backend = "local"

[backends.local]
kind = "openai"
api_key = "unused"
base_url = "http://localhost:11434/v1"
big_model = "qwen2.5-coder:32b"
small_model = "qwen2.5-coder:7b"

[backends.openai]
kind = "openai"
api_key = "env:OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
big_model = "gpt-4o"
small_model = "gpt-4o-mini"

[backends.deepseek]
kind = "openai"
api_key = "sk-deepseek-..."
base_url = "https://api.deepseek.com/v1"
big_model = "deepseek-coder"
small_model = "deepseek-chat"
```
Run with:
```bash
PROXY_CONFIG=config.toml anyllm-proxy
```
Paths map as:
- `http://localhost:3000/v1/messages` -> local (default)
- `http://localhost:3000/openai/v1/messages` -> OpenAI
- `http://localhost:3000/deepseek/v1/messages` -> DeepSeek

### LiteLLM YAML Format
anyllm-proxy accepts LiteLLM `config.yaml` structures directly:
```yaml
# config.yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o-eu
      api_base: https://my-resource.openai.azure.com/
      api_key: os.environ/AZURE_API_KEY
      rpm: 6000
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: os.environ/OPENAI_API_KEY
      rpm: 10000

general_settings:
  master_key: os.environ/LITELLM_MASTER_KEY
```
Run with:
```bash
PROXY_CONFIG=config.yaml anyllm-proxy
```

### LiteLLM Environment Mappings
The proxy supports direct LiteLLM-style environment variable overrides:
- `LITELLM_MASTER_KEY` maps to `PROXY_API_KEYS`
- `LITELLM_CONFIG` maps to `PROXY_CONFIG`
- `AZURE_API_KEY` maps to `AZURE_OPENAI_API_KEY`
- `AZURE_API_BASE` maps to `AZURE_OPENAI_ENDPOINT`
- `AZURE_API_VERSION` maps to `AZURE_OPENAI_API_VERSION`
- `AWS_REGION_NAME` maps to `AWS_REGION`

See [COMPARISON_LITELLM.md](COMPARISON_LITELLM.md) for full details.

---

## Admin API & Virtual Key Management

Virtual keys can be created, updated, and revoked via the admin API.

> [!NOTE]
> All mutating requests (POST/PUT/DELETE) require a CSRF token. Fetch it using `GET /admin/csrf-token` first and pass it in the `X-CSRF-Token` header.

### Create a Virtual Key
```bash
curl -X POST http://localhost:3001/admin/api/keys \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)" \
  -H "X-CSRF-Token: <CSRF_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "description": "dev key",
    "rpm_limit": 60,
    "tpm_limit": 100000,
    "max_budget_usd": 10.00,
    "budget_duration": "monthly",
    "expires_at": "2026-12-31T00:00:00Z",
    "allowed_models": ["claude-*", "gpt-4o"]
  }'
# Response: {"id": 1, "key": "sk-vk...", ...}
```

### Update a Key
```bash
curl -X PUT http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)" \
  -H "X-CSRF-Token: <CSRF_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{"rpm_limit": 120, "max_budget_usd": 20.00}'
```

### Retrieve Spend Metrics
```bash
curl http://localhost:3001/admin/api/keys/1/spend \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)"
```

### Revoke a Key
```bash
curl -X DELETE http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)" \
  -H "X-CSRF-Token: <CSRF_TOKEN>"
```

For more info on using anyllm-proxy as a library or embedding its middleware, see [library-integration.md](library-integration.md).
