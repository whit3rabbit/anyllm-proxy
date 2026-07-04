# Configuration

## Config Directory

anyllm-proxy stores its data in `~/.anyllm/` by default (created with mode `0700` on Unix):

```
~/.anyllm/
  admin.db          SQLite database (keys, models, audit, env imports, config overrides)
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

After env files are loaded, variables previously imported via the admin UI (`POST /admin/api/env/import`, stored in the SQLite `env_import` table) are applied. Env files take precedence over DB imports, and shell environment takes precedence over both.

The data directory path itself is logged at startup:

```
anyllm_proxy: data directory: /home/user/.anyllm
```

## Config File Formats

The `PROXY_CONFIG` variable (or auto-detected `~/.anyllm/config.yaml`) supports three formats, detected by file extension and content:

**YAML files** (`.yaml` or `.yml`):
- If the root key is `models:`, parsed as **simple native format** (supports `tools:` section for tool execution config).
- If the root key is `model_list:`, parsed as **LiteLLM-compatible format** (supports `general_settings.master_key` auto-applied as `PROXY_API_KEYS`, and `litellm_settings.callbacks` for webhook/Langfuse integration).

**Other extensions**: parsed as **TOML** (multi-backend config with `[backends.*]` sections).

**No config file**: falls back to env-var-based single-backend configuration.

Top-level runtime booleans:
- TOML: `redact_secrets = true`
- Simple YAML: `redact_secrets: true` and `anthropic_thinking_repair: true`
- `log_bodies` remains separate and still logs full bodies when enabled.
- `redact_secrets`, `log_bodies`, and `anthropic_thinking_repair` are all
  live-toggleable from the admin UI (Settings tab) or `PUT /admin/api/config`
  without a restart; the config file / env var only sets the startup default.

Simple YAML tool execution also supports opt-in Forge-style tool-call guardrails:
set `tool_execution.guardrails: standard` to nudge noisy shell commands, oversized
write/edit payloads, and grep/glob symbol lookups when an LSP-style tool is available.
If a tool engine is configured, `FORGE_TOOL_CALL_POLICY=standard` can enable the same preset.

**`tool_execution` / `guardrails` are only read from the simple native YAML format**
(top-level `models:` key) or the `FORGE_TOOL_CALL_POLICY` env var. The LiteLLM-compatible
`model_list:` format has no tool sections and silently ignores any `tool_execution`/
`guardrails` block placed in it — `MultiConfig::load()` hard-codes `tool_config: None`
for that branch (`crates/proxy/src/config/multi/loader.rs`). Users on LiteLLM YAML who
need guardrails must use `FORGE_TOOL_CALL_POLICY` instead.

## Model Persistence

Models added via the admin API (`POST /admin/api/models`) are stored in
the SQLite database and survive restarts.

If a YAML config file (`config.yaml` or `PROXY_CONFIG`) also defines
models, those are loaded first as the base layer. Models added through
the admin UI are then added on top. There is no deduplication: if the
same model name + backend + actual model appears in both YAML and the
database, both deployments will be active in the router.

To reset to YAML-only models, remove the admin-added entries via
`DELETE /admin/api/models/{name}`.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--env-file <path>` | Explicit env file path (highest priority for env loading). |
| `--redact-secrets` | Enable upstream JSON/text request secret redaction for this process. Equivalent to `REDACT_SECRETS=true`. |
| `--webui` / `--admin` | Enable the admin web UI on a separate port. |
| `run <command> [args...]` | Start the proxy in the background, pre-configure `ANTHROPIC_*` env vars for the child process, launch `<command>`, and exit when it exits. Useful for wrapping tools like `claude` or `aider`. |

The `WEBUI=1` or `ADMIN=1` environment variables also enable the admin UI (used by docker-entrypoint.sh). `DISABLE_ADMIN=1` overrides both the flag and env var to force-disable.

## Docker

Docker Compose sets explicit paths via environment variables:

```yaml
environment:
  ADMIN_DB_PATH: /data/admin.db
  ADMIN_TOKEN_PATH: /data/.admin_token
  ADMIN_BIND: 0.0.0.0    # required: default 127.0.0.1 is unreachable from host
```

These override the `~/.anyllm/` convention. The home directory layout
does not apply inside containers.

The docker-entrypoint.sh translates `WEBUI=1` or `ADMIN=1` into the `--webui` CLI flag.

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

# Redact detected secrets before forwarding JSON/text requests upstream
anyllm-proxy --redact-secrets

# Wrap a tool (starts proxy, sets ANTHROPIC_* env vars, runs command)
anyllm-proxy run claude
```

## Environment Variables

See [ENV.md](ENV.md) for the full list. Key additions:

| Variable | Default | Description |
|----------|---------|-------------|
| `ANYLLM_HOME` | `~/.anyllm` | Override the data directory path |
| `ADMIN_DB_PATH` | `$ANYLLM_HOME/admin.db` | SQLite database file |
| `ADMIN_TOKEN_PATH` | `$ANYLLM_HOME/.admin_token` | Admin auth token file |
| `ADMIN_BIND` | `127.0.0.1` | Admin UI bind address (set to `0.0.0.0` for Docker) |
