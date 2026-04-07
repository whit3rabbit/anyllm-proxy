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

### Example

```bash
BACKEND=anthropic \
ANTHROPIC_API_KEY=sk-ant-... \
cargo run -p anyllm_proxy
```

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
