# Environment Variables

## Core

These are the variables most users need.

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (empty) | OpenAI API key. Required for proxying requests. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL for the upstream API. Change this to point at compatible APIs or internal proxies. Validated at startup (rejects private IPs, loopback, cloud metadata endpoints). |
| `LISTEN_PORT` | `3000` | Port the proxy listens on. |
| `BIG_MODEL` | `gpt-4o` | OpenAI model used when the Anthropic request specifies a sonnet or opus model. |
| `SMALL_MODEL` | `gpt-4o-mini` | OpenAI model used when the Anthropic request specifies a haiku model. |
| `RUST_LOG` | `info` | Tracing filter. Examples: `debug`, `anthropic_openai_proxy=trace`. |

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
cargo run -p anthropic_openai_proxy
```
