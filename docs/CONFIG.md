# Configuration

## Config Directory

anyllm-proxy stores its data in `~/.anyllm/` by default:

```
~/.anyllm/
  admin.db          SQLite database (keys, models, audit, env imports)
  .admin_token      Auto-generated admin auth token
  .anyllm.env       Environment file (optional)
  config.yaml       Proxy config (optional)
```

Override the entire directory with `ANYLLM_HOME=/path/to/dir`.

Override individual files with their respective env vars (see below).

## File Lookup Order

Each file has a specific resolution order. The first match wins.

| File | Priority 1 (highest) | Priority 2 | Priority 3 |
|------|---------------------|------------|------------|
| Env file | `--env-file` CLI flag | `.anyllm.env` in CWD | `~/.anyllm/.anyllm.env` |
| Config file | `PROXY_CONFIG` env var | `~/.anyllm/config.yaml` | (none) |
| Database | `ADMIN_DB_PATH` env var | `~/.anyllm/admin.db` | |
| Token | `ADMIN_TOKEN_PATH` env var | `~/.anyllm/.admin_token` | |

The data directory path itself is logged at startup:

```
anyllm_proxy: data directory: /home/user/.anyllm
```

## Model Persistence

Models added via the admin API (`POST /admin/api/models`) are stored in
the SQLite database and survive restarts.

If a YAML config file (`config.yaml` or `PROXY_CONFIG`) also defines
models, those are loaded first as the base layer. Models added through
the admin UI are merged on top. On conflict (same model name + backend +
actual model), the YAML definition takes priority.

To reset to YAML-only models, remove the admin-added entries via
`DELETE /admin/api/models/{name}`.

## Docker

Docker Compose sets explicit paths via environment variables:

```yaml
environment:
  ADMIN_DB_PATH: /data/admin.db
  ADMIN_TOKEN_PATH: /data/.admin_token
```

These override the `~/.anyllm/` convention. The home directory layout
does not apply inside containers.

## Quick Start

### Minimal .anyllm.env

```env
# OpenAI (remote)
OPENAI_API_KEY=sk-your-key-here
PROXY_API_KEYS=my-proxy-key

# Or: Ollama (local, no API key)
# OPENAI_BASE_URL=http://localhost:11434/v1
# PROXY_OPEN_RELAY=true
```

Save this to `~/.anyllm/.anyllm.env` and the proxy picks it up
automatically on next start.

### Startup options

```bash
# Auto-loads ~/.anyllm/.anyllm.env if present
anyllm-proxy

# Explicit env file
anyllm-proxy --env-file /path/to/my.env

# With admin UI
anyllm-proxy --webui

# Both
anyllm-proxy --webui --env-file /path/to/my.env
```

## Environment Variables

See [ENV.md](ENV.md) for the full list. Key additions:

| Variable | Default | Description |
|----------|---------|-------------|
| `ANYLLM_HOME` | `~/.anyllm` | Override the data directory path |
| `ADMIN_DB_PATH` | `$ANYLLM_HOME/admin.db` | SQLite database file |
| `ADMIN_TOKEN_PATH` | `$ANYLLM_HOME/.admin_token` | Admin auth token file |
