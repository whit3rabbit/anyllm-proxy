# Environment Variables

## Env Files

Instead of setting variables in the shell, you can store them in a `.env` file and load it at startup.

**Auto-load:** If `.anyllm.env` exists in the current directory, it is loaded automatically. If not found, `~/.anyllm/.anyllm.env` is checked.

**Explicit flag:**
```bash
anyllm_proxy --env-file ~/configs/deepseek.env
```

**File format** (`KEY=VALUE`, Docker `--env-file` compatible):
```env
# Comments are supported
OPENAI_API_KEY=sk-...
OPENAI_BASE_URL=https://api.deepseek.com/v1
BIG_MODEL=deepseek-coder
SMALL_MODEL=deepseek-chat
export LISTEN_PORT=3000   # export prefix is also accepted
```

Rules:
- Lines starting with `#` are ignored.
- Values may be optionally quoted with `"double"` or `'single'` quotes.
- Double-quoted values interpret backslash escapes (`\n`, `\t`, `\r`, `\\`, `\"`).
- Single-quoted values are literal (no escape processing, matching bash behavior).
- Environment variables already set in the shell take precedence over the file.
- Variables previously imported via the admin UI (stored in SQLite) are applied after env files, with env files taking precedence.
- Use `docker run --env-file <path>` to pass the same file to a container.

The admin UI (Settings tab) has an **Export .env** button that generates a template from the current running configuration.

---

## Core

These are the variables most users need.

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (empty) | OpenAI API key. Required for the default `openai` backend. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL for the upstream API. Change this to point at compatible APIs (Ollama, OpenRouter, etc.). Validated at startup (rejects private IPs, loopback, cloud metadata endpoints). |
| `OPENAI_API_FORMAT` | `chat` | Which OpenAI API format to use. `chat` (default) for Chat Completions, `responses` for the Responses API. Only relevant when `BACKEND=openai`. |
| `BACKEND` | `openai` | Which upstream backend to target. Valid values: `openai`, `azure`, `vertex`, `gemini`, `anthropic`, `bedrock`. |
| `LISTEN_PORT` | `3000` | Port the proxy listens on. |
| `BIG_MODEL` | (per backend) | Model used when the request specifies a sonnet or opus model. Defaults: `gpt-4o` (openai/azure), `gemini-2.5-pro` (vertex/gemini), Bedrock model ID (bedrock). Not used for `anthropic` backend (passthrough). |
| `SMALL_MODEL` | (per backend) | Model used when the request specifies a haiku model. Defaults: `gpt-4o-mini` (openai/azure), `gemini-2.5-flash` (vertex/gemini), Bedrock model ID (bedrock). |
| `RUST_LOG` | `info` | Tracing filter. Examples: `debug`, `anyllm_proxy=trace`. |
| `LOG_BODIES` | `false` | Log request/response bodies at debug level. Set to `true` or `1`. **Warning:** may expose sensitive data (prompts, API keys, PII). |
| `REDACT_SECRETS` | `false` | Scan upstream JSON/text request payloads and replace detected secrets before forwarding. Set to `true` or `1`. Also available as `--redact-secrets` and in the admin UI. |
| `ANYLLM_DEGRADATION_WARNINGS` | `false` | Expose `x-anyllm-degradation` response header when features are silently dropped during translation. Set to `true` or `1`. Automatically enabled when `PROXY_CONFIG` is set. |
| `DISABLE_ADMIN` | (unset) | Set to `1`, `true`, or `yes` to force-disable the admin web interface even when `--webui` is passed. Useful in automated/container environments. |

## Auth

| Variable | Default | Description |
|----------|---------|-------------|
| `PROXY_API_KEYS` | (unset) | Comma-separated list of allowed API keys. Clients must send one of these as their Bearer token. If unset and `PROXY_OPEN_RELAY` is not set, all requests are rejected with 401. |
| `PROXY_OPEN_RELAY` | (unset) | Set to `true` or `1` to accept any non-empty API key. **Local dev only.** Logged as an error when bound to a non-loopback address. |
| `PROXY_CONFIG` | (unset) | Path to a config file (simple YAML, LiteLLM YAML, or TOML). Auto-detected from `~/.anyllm/config.yaml` if not set. See [CONFIG.md](CONFIG.md). |

## Network / Security

| Variable | Default | Description |
|----------|---------|-------------|
| `IP_ALLOWLIST` | (unset) | Comma-separated list of allowed client IPs or CIDR ranges (e.g. `10.0.0.0/8,192.168.1.5`). When set, requests from other IPs are rejected. |
| `TRUST_PROXY_HEADERS` | `false` | Trust `X-Forwarded-For` and `X-Real-IP` headers for client IP resolution. Set to `true` or `1` when behind a reverse proxy. |
| `REQUEST_TIMEOUT_SECS` | `900` | Wall-clock cap (seconds) for streaming responses. 0 = disabled. |
| `OMIT_STREAM_OPTIONS` | `false` | Strip `stream_options` from streaming requests. Needed for local LLMs (older Ollama, text-generation-webui, LM Studio) that reject unknown fields with HTTP 400. |

## Tool Guardrails

These apply only when a config file initializes the tool engine through `tool_execution`, `builtin_tools`, or `mcp_servers`.

| Variable | Default | Description |
|----------|---------|-------------|
| `FORGE_TOOL_CALL_POLICY` | `disabled` | Set to `standard` to enable Forge-style advisory guardrails for model-produced tool calls. YAML `tool_execution.guardrails` takes precedence. |

## OIDC / JWT Authentication (optional)

When `OIDC_ISSUER_URL` is set, the proxy discovers the OIDC configuration and loads JWKS. Tokens that look like JWTs are validated against the JWKS before falling through to key-based auth.

| Variable | Default | Description |
|----------|---------|-------------|
| `OIDC_ISSUER_URL` | (unset) | OIDC issuer URL for JWT validation (e.g. `https://accounts.google.com`). Enables OIDC authentication when set. |
| `OIDC_AUDIENCE` | (issuer URL) | Expected audience claim in JWTs. Defaults to the issuer URL if not set. |

## AWS Bedrock

Set `BACKEND=bedrock` to route through AWS Bedrock. The proxy sends Anthropic Messages API format directly to Bedrock (no OpenAI translation). Requests are signed with AWS SigV4.

| Variable | Default | Description |
|----------|---------|-------------|
| `AWS_REGION` | (required) | AWS region, e.g. `us-east-1`. |
| `AWS_ACCESS_KEY_ID` | (required) | AWS access key ID for SigV4 signing. |
| `AWS_SECRET_ACCESS_KEY` | (required) | AWS secret access key for SigV4 signing. |
| `AWS_SESSION_TOKEN` | (optional) | Temporary session token for STS credentials. |
| `BIG_MODEL` | `anthropic.claude-sonnet-4-20250514-v1:0` | Bedrock model ID for sonnet/opus requests. |
| `SMALL_MODEL` | `anthropic.claude-haiku-4-5-20251001-v1:0` | Bedrock model ID for haiku requests. |

### Example

```bash
BACKEND=bedrock \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=wJalr... \
cargo run -p anyllm_proxy
```

### Streaming

Bedrock streaming uses AWS Event Stream binary framing instead of SSE. The proxy decodes Event Stream frames and re-emits them as standard SSE events, so downstream clients see the same Anthropic SSE format as with other backends.

---

## Azure OpenAI

Set `BACKEND=azure` to route through Azure OpenAI Service. The request/response format is identical to standard OpenAI Chat Completions; only the URL scheme and auth header differ.

| Variable | Default | Description |
|----------|---------|-------------|
| `AZURE_OPENAI_API_KEY` | (required) | Azure OpenAI API key. Sent as `api-key` header. |
| `AZURE_OPENAI_ENDPOINT` | (required) | Full Azure resource endpoint, e.g. `https://my-resource.openai.azure.com`. Accepts sovereign cloud URLs. |
| `AZURE_OPENAI_DEPLOYMENT` | (required) | Deployment name (the model deployment you created in Azure portal). |
| `AZURE_OPENAI_API_VERSION` | `2024-10-21` | Azure API version string appended as `?api-version=` query parameter. |

The proxy constructs the full URL as:
```
{AZURE_OPENAI_ENDPOINT}/openai/deployments/{AZURE_OPENAI_DEPLOYMENT}/chat/completions?api-version={AZURE_OPENAI_API_VERSION}
```

### Example

```bash
BACKEND=azure \
AZURE_OPENAI_API_KEY=abc123 \
AZURE_OPENAI_ENDPOINT=https://my-resource.openai.azure.com \
AZURE_OPENAI_DEPLOYMENT=gpt-4o \
cargo run -p anyllm_proxy
```

---

## Google Vertex AI

Set `BACKEND=vertex` to route through Google Vertex AI. The proxy constructs the Vertex AI endpoint URL from the project and region, then forwards via the OpenAI-compatible API.

| Variable | Default | Description |
|----------|---------|-------------|
| `VERTEX_PROJECT` | (required) | GCP project ID. |
| `VERTEX_REGION` | (required) | GCP region, e.g. `us-central1`. |
| `VERTEX_API_KEY` | (one required) | Google API key for authentication. Either this or `GOOGLE_ACCESS_TOKEN` must be set. |
| `GOOGLE_ACCESS_TOKEN` | (one required) | OAuth2 access token for authentication. Alternative to `VERTEX_API_KEY`. |
| `BIG_MODEL` | `gemini-2.5-pro` | Model for sonnet/opus requests. |
| `SMALL_MODEL` | `gemini-2.5-flash` | Model for haiku requests. |

The proxy constructs the endpoint as:
```
https://{VERTEX_REGION}-aiplatform.googleapis.com/v1/projects/{VERTEX_PROJECT}/locations/{VERTEX_REGION}/endpoints/openapi
```

### Example

```bash
BACKEND=vertex \
VERTEX_PROJECT=my-project \
VERTEX_REGION=us-central1 \
VERTEX_API_KEY=AIza... \
cargo run -p anyllm_proxy
```

---

## Google Gemini

Set `BACKEND=gemini` to route through the Gemini API (generativelanguage.googleapis.com). Uses the OpenAI-compatible endpoint.

| Variable | Default | Description |
|----------|---------|-------------|
| `GEMINI_API_KEY` | (required) | Gemini API key. Sent as `x-goog-api-key` header. |
| `GEMINI_BASE_URL` | `https://generativelanguage.googleapis.com/v1beta` | Base URL. The proxy appends `/openai` to reach the OpenAI-compatible endpoint. |
| `BIG_MODEL` | `gemini-2.5-pro` | Model for sonnet/opus requests. |
| `SMALL_MODEL` | `gemini-2.5-flash` | Model for haiku requests. |

### Example

```bash
BACKEND=gemini \
GEMINI_API_KEY=AIza... \
cargo run -p anyllm_proxy
```

---

## Anthropic Passthrough

Set `BACKEND=anthropic` to forward Anthropic Messages API requests directly to the Anthropic API without any translation. Model names are passed through unchanged (no BIG_MODEL/SMALL_MODEL mapping).

| Variable | Default | Description |
|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | (required) | Anthropic API key. |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Base URL for the Anthropic API. |
| `ANTHROPIC_THINKING_REPAIR` | `false` | Repair corrupted `thinking`/`redacted_thinking` blocks in the last assistant message of `/v1/messages` requests before forwarding upstream. See below. |
| `ANTHROPIC_FORWARD_CLIENT_AUTH` | `false` | Forward the client's own `x-api-key`/`Authorization` header upstream verbatim instead of `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`. See below. |

### Example

```bash
BACKEND=anthropic \
ANTHROPIC_API_KEY=sk-ant-... \
cargo run -p anyllm_proxy
```

### Thinking-block repair (`ANTHROPIC_THINKING_REPAIR=true`)

Clients that replay conversation history (e.g. Claude Code) can corrupt the
`thinking`/`redacted_thinking` blocks in the last assistant message — merged
text from interleaved streams, dropped `redacted_thinking` blocks that never
get persisted to disk, reordered blocks. The Anthropic API validates those
blocks byte-exactly against their signatures, so any mutation produces a
repeating 400 until the client's context is cleared.

With this flag set, the proxy records every response's content blocks
(text, signatures, `redacted_thinking` data, `tool_use` ownership) as ground
truth, then on each outgoing request verifies and repairs only the *last*
assistant message against it: byte-identical blocks pass through untouched,
blocks with a known signature but mutated text are restored to the recorded
original, and blocks belonging to a different recorded message ("intruder"
blocks) are dropped. Messages before the last assistant one are never
touched, so prompt-cache prefixes are preserved.

The ground-truth store is in-memory only (bounded, no persistence). On
proxy restart it starts empty; requests are forwarded unrepaired until a
fresh response is recorded, then repair resumes on the next turn. Off by
default; only takes effect for `BACKEND=anthropic` passthrough (`/v1/messages`).

This can also be toggled live from the admin UI (Settings tab) or via
`PUT /admin/api/config` with `{"anthropic_thinking_repair": true|false}` — no
restart required. The env var only sets the value at startup; an admin-UI
change takes effect immediately and persists across restarts via SQLite until
reset.

### Forwarding the client's own credential (`ANTHROPIC_FORWARD_CLIENT_AUTH=true`)

By default the proxy always sends **its own** `ANTHROPIC_API_KEY`/
`ANTHROPIC_AUTH_TOKEN` to the real Anthropic API — the client's incoming
`x-api-key`/`Authorization` header is only ever checked against the proxy's
own inbound auth (`PROXY_API_KEYS`/`PROXY_OPEN_RELAY`) and then discarded.
Setting this flag instead forwards that exact header — same name, same
value, byte-for-byte, no re-shaping — upstream in place of the operator's
configured credential. This lets Claude Code use its own Pro/Max
subscription OAuth session directly through the proxy, without a separate
`claude setup-token` step.

Since the credential that authenticates a request into the proxy becomes the
literal credential sent to Anthropic, this only makes sense for a
single-key/BYOK deployment where those two are meant to be the same thing.
It is automatically skipped (the operator's own credential is used instead,
regardless of the flag) for any request authenticated via a virtual key or
OIDC/JWT — a virtual key is deliberately not a real Anthropic credential, and
forwarding a JWT upstream would never work. A client that authenticated via
the Gemini-CLI-compatible `x-goog-api-key` header has its value forwarded
renamed to `x-api-key` (the only credential header name Anthropic itself
understands), not literally as `x-goog-api-key`.

At startup, the proxy refuses to start with this flag on if `PROXY_API_KEYS`
has 2+ distinct entries and `PROXY_OPEN_RELAY` is not set, since that
combination would let different callers each redirect the upstream Anthropic
credential. The same rule is enforced live: this flag is toggleable from the
admin UI (**Settings**) or `PUT /admin/api/config` with no restart, and that
route rejects the same misconfigured combination with a 400 rather than
silently accepting it.

```bash
BACKEND=anthropic \
ANTHROPIC_AUTH_TOKEN=$(claude setup-token) \
ANTHROPIC_FORWARD_CLIENT_AUTH=true \
PROXY_OPEN_RELAY=true \
anyllm_proxy
```

Only active for `BACKEND=anthropic` passthrough (`/v1/messages` and the
generic Anthropic-native catch-all route). Off by default. Applies uniformly
to every `BackendKind::Anthropic` backend in a multi-backend deployment (one
shared runtime setting, like `ANTHROPIC_THINKING_REPAIR`) rather than being
configurable per backend.

---

## Third-party OpenAI-compatible providers

Any LiteLLM provider id from the built-in catalog can be used as a `BACKEND` value. These providers use the OpenAI Chat Completions protocol and route through the same HTTP client as `BACKEND=openai`. Legacy local IDs such as `gmi_cloud`, `public_ai`, `zhipuai`, `ai_ml_api`, `github`, `jina`, `exa`, and `stability_ai` are accepted only as migration aliases.

**Resolution order:**
1. `BACKEND=<provider_id>` — e.g. `BACKEND=groq`
2. Base URL: `OPENAI_BASE_URL` env var (if set) overrides the provider default; otherwise the catalog default is used. Known providers without a safe global default require `OPENAI_BASE_URL`.
3. API key: `OPENAI_API_KEY` (if set) takes precedence; otherwise the provider-specific key var is used (e.g. `GROQ_API_KEY`).

**Example (Groq):**
```bash
BACKEND=groq \
GROQ_API_KEY=gsk_... \
PROXY_OPEN_RELAY=true \
cargo run -p anyllm_proxy
```

### Popular cloud providers

| `BACKEND` value | API key env var(s) | Default base URL |
|---|---|---|
| `groq` | `GROQ_API_KEY` | `https://api.groq.com/openai/v1` |
| `together_ai` | `TOGETHER_API_KEY`, `TOGETHERAI_API_KEY` | `https://api.together.xyz/v1` |
| `openrouter` | `OPENROUTER_API_KEY` | `https://openrouter.ai/api/v1` |
| `fireworks_ai` | `FIREWORKS_API_KEY` | `https://api.fireworks.ai/inference/v1` |
| `mistral` | `MISTRAL_API_KEY` | `https://api.mistral.ai/v1` |
| `codestral` | `CODESTRAL_API_KEY` | `https://codestral.mistral.ai/v1` |
| `perplexity` | `PERPLEXITYAI_API_KEY`, `PERPLEXITY_API_KEY` | `https://api.perplexity.ai` |
| `deepseek` | `DEEPSEEK_API_KEY` | `https://api.deepseek.com` |
| `cerebras` | `CEREBRAS_API_KEY` | `https://api.cerebras.ai/v1` |
| `xai` | `XAI_API_KEY` | `https://api.x.ai/v1` |
| `nvidia_nim` | `NVIDIA_NIM_API_KEY` | `https://integrate.api.nvidia.com/v1` |
| `sambanova` | `SAMBANOVA_API_KEY` | `https://api.sambanova.ai/v1` |
| `nebius` | `NEBIUS_API_KEY` | `https://api.studio.nebius.ai/v1` |
| `deepinfra` | `DEEPINFRA_API_KEY` | `https://api.deepinfra.com/v1/openai` |
| `novita` | `NOVITA_API_KEY` | `https://api.novita.ai/v3/openai` |
| `hyperbolic` | `HYPERBOLIC_API_KEY` | `https://api.hyperbolic.xyz/v1` |
| `lambda_ai` | `LAMBDA_API_KEY` | `https://api.lambdalabs.com/v1` |
| `nscale` | `NSCALE_API_KEY` | `https://inference.nscale.com/v1` |
| `featherless_ai` | `FEATHERLESS_API_KEY` | `https://api.featherless.ai/v1` |
| `friendliai` | `FRIENDLIAI_TOKEN` | `https://api.friendli.ai/serverless/v1` |
| `replicate` | `REPLICATE_API_KEY` | `https://openai-compat.replicate.com/v1` |
| `cohere_chat` | `COHERE_API_KEY` | `https://api.cohere.com/compatibility/v1` |
| `ai21` | `AI21_API_KEY` | `https://api.ai21.com/studio/v1` |
| `anyscale` | `ANYSCALE_API_KEY` | `https://api.endpoints.anyscale.com/v1` |
| `aleph_alpha` | `ALEPH_ALPHA_API_KEY` | `https://api.aleph-alpha.com` |
| `nlp_cloud` | `NLP_CLOUD_API_KEY` | `https://api.nlpcloud.io` |
| `clarifai` | `CLARIFAI_API_KEY` | `https://api.clarifai.com/v2` |
| `predibase` | `PREDIBASE_API_KEY` | `https://serving.app.predibase.com` |
| `voyage` | `VOYAGE_API_KEY` | `https://api.voyageai.com/v1` (embeddings only) |
| `jina_ai` | `JINA_AI_API_KEY` | `https://api.jina.ai/v1` (embeddings/rerank) |
| `github_copilot` | `GITHUB_TOKEN` | `https://models.github.ai/inference` |
| `chutes` | `CHUTES_API_KEY` | `https://llm.chutes.ai/v1` |
| `gmi` | `GMI_CLOUD_API_KEY` | `https://api.gmi-serving.com/v1` |
| `meta_llama` | `META_LLAMA_API_KEY` | `https://www.llama.com/api/v1` |
| `aiml` | `AIML_API_KEY` | `https://api.aimlapi.com/v1` |
| `morph` | `MORPH_API_KEY` | `https://api.morphllm.com/v1` |
| `galadriel` | `GALADRIEL_API_KEY` | `https://api.galadriel.com/v1` |
| `nanogpt` | `NANOGPT_API_KEY` | `https://nano-gpt.com/api/v1` |
| `bytez` | `BYTEZ_KEY` | `https://api.bytez.com/models/v2` |
| `publicai` | `PUBLIC_AI_API_KEY` | `https://api.publicai.co/v1` |

### Regional / specialized

| `BACKEND` value | API key env var(s) | Default base URL |
|---|---|---|
| `moonshot` | `MOONSHOT_API_KEY` | `https://api.moonshot.cn/v1` |
| `volcengine` | `VOLCENGINE_API_KEY` | `https://ark.cn-beijing.volces.com/api/v3` |
| `minimax` | `MINIMAX_API_KEY` | `https://api.minimax.chat/v1` |
| `zai` | `ZHIPUAI_API_KEY` | `https://open.bigmodel.cn/api/paas/v4` |
| `dashscope` | `DASHSCOPE_API_KEY` | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| `xiaomi_mimo` | `XIAOMI_MIMO_API_KEY` | `https://api.mimo.chat/v1` |
| `gradient_ai` | `GRADIENT_ACCESS_TOKEN` | `https://api.gradient.ai` |

### Per-deployment (must set `OPENAI_BASE_URL`)

These providers require a workspace/account-specific URL set via `OPENAI_BASE_URL`.

| `BACKEND` value | API key env var(s) | Notes |
|---|---|---|
| `databricks` | `DATABRICKS_API_KEY` | Set `OPENAI_BASE_URL` to your workspace serving endpoint |
| `hosted_vllm` | `VLLM_API_KEY` | Set `OPENAI_BASE_URL` to your vLLM server URL |
| `huggingface` | `HUGGINGFACE_API_KEY`, `HF_TOKEN` | Set `OPENAI_BASE_URL` to your HF Inference Endpoint |
| `scaleway` | `SCW_SECRET_KEY` | Set `OPENAI_BASE_URL` to your Scaleway Inference endpoint |
| `baseten` | `BASETEN_API_KEY` | Set `OPENAI_BASE_URL` to your Baseten deployment URL |
| `azure_ai` | `AZURE_AI_API_KEY`, `AZURE_AI_API_BASE` | Azure AI Foundry; also set `OPENAI_BASE_URL=<AZURE_AI_API_BASE>` |
| `watsonx` | `WATSONX_API_KEY`, `WATSONX_URL` | IBM WatsonX; also set `OPENAI_BASE_URL=<WATSONX_URL>` |
| `cloudflare` | `CLOUDFLARE_API_KEY`, `CLOUDFLARE_ACCOUNT_ID` | Set `OPENAI_BASE_URL` to your account endpoint |
| `snowflake` | `SNOWFLAKE_JWT`, `SNOWFLAKE_ACCOUNT_ID` | Set `OPENAI_BASE_URL` to your Snowflake Cortex endpoint |
| `xinference` | `XINFERENCE_SERVER_URL` | Set `OPENAI_BASE_URL=<XINFERENCE_SERVER_URL>` |
| `ovhcloud` | `OVH_AI_ENDPOINTS_ACCESS_TOKEN` | Set `OPENAI_BASE_URL` to your OVHcloud endpoint |
| `wandb` | `WANDB_API_KEY` | Set `OPENAI_BASE_URL` to your W&B Inference project URL |

### Self-hosted / local (no key required)

| `BACKEND` value | Default base URL | Notes |
|---|---|---|
| `ollama` | `http://localhost:11434/v1` | Override with `OPENAI_BASE_URL` for remote Ollama |
| `lm_studio` | `http://localhost:1234/v1` | LM Studio local server |
| `llamafile` | `http://localhost:8080` | llamafile server |
| `lemonade` | `http://localhost:8000` | Lemonade local server |
| `docker_model_runner` | `http://localhost:12434/engines/llama.cpp/v1` | Docker Model Runner |
| `infinity` | `http://localhost:7997` | Infinity embeddings server |
| `petals` | `http://localhost:8080` | Petals distributed inference |
| `triton` | (none — set `OPENAI_BASE_URL`) | NVIDIA Triton Inference Server |

> **Note:** `sagemaker` appears in the provider catalog but uses a custom AWS signing protocol not yet routed through `BACKEND=sagemaker`. Use `BACKEND=bedrock` for AWS-hosted Anthropic models instead.

---

## mTLS Client Certificates

Most users do not need these. They configure mutual TLS (mTLS) on the **outbound** connection from the proxy to the backend endpoint. Use them when the backend requires a client certificate for authentication, or uses a private CA that is not in the system trust store.

These variables do not affect the proxy's own listener. The proxy always serves plain HTTP. For inbound TLS termination, place a reverse proxy (nginx, caddy, etc.) in front.

| Variable | Default | Description |
|----------|---------|-------------|
| `TLS_CLIENT_CERT_P12` | (unset) | Path to a PKCS#12 (.p12 or .pfx) client certificate file. When set, the proxy presents this certificate during the TLS handshake with the backend. |
| `TLS_CLIENT_CERT_PASSWORD` | (unset) | Password to decrypt the P12 file. **Required** if `TLS_CLIENT_CERT_P12` is set. The proxy will refuse to start if the P12 is set without a password. |
| `TLS_CA_CERT` | (unset) | Path to a PEM-encoded CA certificate. Added to the trust store for verifying the backend's server certificate. Use this when the backend uses a private or self-signed CA. |

All three are optional. When unset, the proxy connects using the system's default TLS configuration and trust store.

### Validation

All certificate files are read and validated at startup. The proxy will panic with a descriptive error if:

- The P12 file does not exist or cannot be read.
- The P12 password is wrong or the file is corrupt.
- The CA certificate file does not exist or is not valid PEM.
- `TLS_CLIENT_CERT_P12` is set without `TLS_CLIENT_CERT_PASSWORD`.

### Example

```bash
OPENAI_API_KEY=sk-... \
OPENAI_BASE_URL=https://internal-llm.corp.example.com \
TLS_CLIENT_CERT_P12=/etc/proxy/client.p12 \
TLS_CLIENT_CERT_PASSWORD=changeit \
TLS_CA_CERT=/etc/proxy/corp-ca.pem \
cargo run -p anyllm_proxy
```

---

## Admin Web UI

The admin web interface is **opt-in**. Start the proxy with `--webui` or `--admin` to enable it. The `WEBUI=1` or `ADMIN=1` environment variables also work (used by docker-entrypoint.sh).

```bash
anyllm_proxy --webui
```

The dashboard binds to `127.0.0.1:3001` by default (not externally accessible). It shows live request logs, latency percentiles, error rates, per-backend metrics, and lets you change log level and model mappings without restarting the server. The Settings tab also displays all active environment variables (secrets are masked).

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_PORT` | `3001` | Port for the admin dashboard. Must differ from `LISTEN_PORT`. |
| `ADMIN_BIND` | `127.0.0.1` | Bind address for the admin dashboard. Set to `0.0.0.0` to make it reachable from outside the host (required in Docker). |
| `ADMIN_TOKEN` | (generated) | Bearer token for the admin API. If unset, a random 256-bit hex token is generated at startup and written to `ADMIN_TOKEN_PATH`. On non-Unix platforms, auto-generation is not supported; set this explicitly. |
| `ADMIN_TOKEN_PATH` | `~/.anyllm/.admin_token` | File path where the generated admin token is written. Permissions are set to `0600` on Unix. |
| `ADMIN_DB_PATH` | `~/.anyllm/admin.db` | SQLite database path for request logging, config overrides, virtual keys, and model deployments. Config overrides survive restarts. |
| `ADMIN_LOG_RETENTION_DAYS` | `7` | Days to retain request log entries before automatic purge. |
| `DISABLE_ADMIN` | (unset) | Set to `1`, `true`, or `yes` to force-disable the admin server even when `--webui` is passed. Useful in container deployments where the flag might be baked into the entrypoint. |

### Token security

The admin token is written to `ADMIN_TOKEN_PATH` (default `~/.anyllm/.admin_token`) rather than stderr, because container log drivers capture stderr and persist it in centralized logging systems. On Unix, the file is created with mode `0600`. The token is printed to stdout for easy copy on first launch.

In production, set `ADMIN_TOKEN` explicitly:

```bash
ADMIN_TOKEN=$(openssl rand -hex 32) anyllm_proxy --webui
```

### Example

```bash
# Proxy + admin UI on a custom port with a fixed token
ADMIN_PORT=4000 \
ADMIN_TOKEN=my-secret-token \
ADMIN_DB_PATH=/var/lib/anyllm/admin.db \
anyllm_proxy --webui
# Open: http://127.0.0.1:4000/admin/?token=my-secret-token
```

---

## Webhooks / Callbacks

| Variable | Default | Description |
|----------|---------|-------------|
| `WEBHOOK_URLS` | (unset) | Comma-separated list of webhook URLs to POST request completion events to. |
| `BATCH_WEBHOOK_URLS` | (unset) | Comma-separated global webhook URLs for batch API job completions. Only active when admin is enabled. |
| `BATCH_WEBHOOK_SIGNING_SECRET` | (unset) | Secret for HMAC-signing batch webhook payloads. |

---

## Langfuse Integration (optional)

Send LLM generation events to Langfuse's batch ingestion API. Activated when both `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` are set, or when `"langfuse"` appears in `litellm_settings.callbacks` in a LiteLLM config file.

| Variable | Default | Description |
|----------|---------|-------------|
| `LANGFUSE_PUBLIC_KEY` | (required) | Langfuse public key. |
| `LANGFUSE_SECRET_KEY` | (required) | Langfuse secret key. |
| `LANGFUSE_HOST` | `https://cloud.langfuse.com` | Langfuse API host. Validated against SSRF (rejects private IPs). |

---

## Distributed Rate Limiting (optional)

Requires building with `--features redis`. When `REDIS_URL` is set, RPM/TPM rate limit checks are performed against Redis so multiple proxy instances share rate limit state.

| Variable | Default | Description |
|----------|---------|-------------|
| `REDIS_URL` | (unset) | Redis connection URL (e.g. `redis://localhost:6379`). Enables distributed rate limiting when set. |
| `RATE_LIMIT_FAIL_POLICY` | `open` | Behavior when Redis is unreachable: `open` (allow requests through) or `closed`/`deny` (reject requests). |

---

## Semantic Cache (optional)

Requires building with `--features qdrant`. Uses Qdrant for embedding-based response caching.

| Variable | Default | Description |
|----------|---------|-------------|
| `QDRANT_URL` | (unset) | Qdrant connection URL. Enables semantic caching when set. |
| `QDRANT_COLLECTION` | (unset) | Qdrant collection name for cached responses. |

---

## Cost Tracking

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_PRICING_FILE` | (embedded) | Path to a JSON file overriding the embedded model pricing data. |

---

## OpenTelemetry (optional)

Trace export is opt-in. Build with the `otel` cargo feature to enable it:

```bash
cargo build -p anyllm_proxy --features otel
```

When the feature is enabled, the proxy initializes an OTLP span exporter that sends traces over HTTP/protobuf. The OTLP SDK reads configuration from standard environment variables; no proxy-specific config is needed.

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4318` | OTLP collector endpoint (HTTP). |
| `OTEL_SERVICE_NAME` | `unknown_service` | Service name attached to all exported spans. Set this to `anyllm-proxy` or your deployment name. |
| `OTEL_TRACES_SAMPLER` | `parentbased_always_on` | Sampling strategy. Common values: `always_on`, `always_off`, `traceidratio` (pair with `OTEL_TRACES_SAMPLER_ARG`). |
| `OTEL_TRACES_SAMPLER_ARG` | (none) | Argument for the sampler, e.g. `0.1` for 10% sampling with `traceidratio`. |

When built without the `otel` feature (the default), none of these variables have any effect and there is zero runtime overhead.

---

## LiteLLM Environment Variable Aliases

For compatibility with LiteLLM configurations, the proxy recognizes these aliases. Aliases only take effect when the target variable is not already set.

| LiteLLM Variable | Maps To |
|------------------|---------|
| `LITELLM_MASTER_KEY` | `PROXY_API_KEYS` |
| `LITELLM_CONFIG` | `PROXY_CONFIG` |
| `AZURE_API_KEY` | `AZURE_OPENAI_API_KEY` |
| `AZURE_API_BASE` | `AZURE_OPENAI_ENDPOINT` |
| `AZURE_API_VERSION` | `AZURE_OPENAI_API_VERSION` |
| `AWS_REGION_NAME` | `AWS_REGION` |
| `LITELLM_IP_ALLOWLIST` | `IP_ALLOWLIST` |
