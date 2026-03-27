# Environment Variables

## Env Files

Instead of setting variables in the shell, you can store them in a `.env` file and load it at startup.

**Auto-load:** If `.anyllm.env` exists in the current directory, it is loaded automatically.

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
- Environment variables already set in the shell take precedence over the file.
- Use `docker run --env-file <path>` to pass the same file to a container.

The admin UI (Settings tab) has an **Export .env** button that generates a template from the current running configuration.

---

## Core

These are the variables most users need.

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (empty) | OpenAI API key. Required for proxying requests. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL for the upstream API. Change this to point at compatible APIs or internal proxies. Validated at startup (rejects private IPs, loopback, cloud metadata endpoints). |
| `LISTEN_PORT` | `3000` | Port the proxy listens on. |
| `BIG_MODEL` | `gpt-4o` | OpenAI model used when the Anthropic request specifies a sonnet or opus model. |
| `SMALL_MODEL` | `gpt-4o-mini` | OpenAI model used when the Anthropic request specifies a haiku model. |
| `RUST_LOG` | `info` | Tracing filter. Examples: `debug`, `anyllm_proxy=trace`. |
| `DISABLE_ADMIN` | (unset) | Set to `1`, `true`, or `yes` to force-disable the admin web interface even when `--webui` is passed. Useful in automated/container environments. |

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

The admin web interface is **opt-in**. Start the proxy with `--webui` or `--admin` to enable it.

```bash
anyllm_proxy --webui
```

The dashboard binds to `localhost:3001` only (never externally accessible). It shows live request logs, latency percentiles, error rates, per-backend metrics, and lets you change log level and model mappings without restarting the server. The Settings tab also displays all active environment variables (secrets are masked).

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_PORT` | `3001` | Port for the admin dashboard. Must differ from `LISTEN_PORT`. |
| `ADMIN_TOKEN` | (generated) | Bearer token for the admin API. If unset, a random UUID is generated at startup and written to `ADMIN_TOKEN_PATH`. |
| `ADMIN_TOKEN_PATH` | `.admin_token` | File path where the generated admin token is written. Permissions are set to `0600` on Unix. |
| `ADMIN_DB_PATH` | `admin.db` | SQLite database path for request logging and config overrides (model mappings, log level). Config overrides survive restarts. |
| `ADMIN_LOG_RETENTION_DAYS` | `7` | Days to retain request log entries before automatic purge. |
| `DISABLE_ADMIN` | (unset) | Set to `1`, `true`, or `yes` to force-disable the admin server even when `--webui` is passed. Useful in container deployments where the flag might be baked into the entrypoint. |

### Token security

The admin token is printed to `ADMIN_TOKEN_PATH` (default `.admin_token`) rather than stdout/stderr, because container log drivers capture stderr and persist it in centralized logging systems. On Unix, the file is created with mode `0600`.

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
