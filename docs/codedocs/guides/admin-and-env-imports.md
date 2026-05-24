---
title: "Admin UI And Env Imports"
description: "Enable the admin server, authenticate with the generated token, and use env import/export safely."
---

This guide covers the mutable operations path: local admin access, generated tokens, and importing `.anyllm.env` content into SQLite so it survives restarts.

<Steps>
<Step>
### Start the proxy with the admin UI

```bash
anyllm_proxy --webui
```

Expected output includes the localhost-only admin port:

```text
Proxy:     http://localhost:3000
Admin UI:  http://127.0.0.1:3001/admin/?token=...
```
</Step>
<Step>
### Read the generated admin token

```bash
cat ~/.anyllm/.admin_token
```

Open the admin URL with `?token=...`, or send the same token as `Authorization: Bearer <token>` when calling the admin API.
</Step>
<Step>
### Import a saved env file

Create a file:

```bash
OPENAI_BASE_URL=http://localhost:11434/v1
OPENAI_API_KEY=unused
BIG_MODEL=qwen2.5-coder:32b
SMALL_MODEL=qwen2.5-coder:32b
PROXY_API_KEYS=proxy-user
```

Then import it:

```bash
curl -X POST 'http://127.0.0.1:3001/admin/api/env/import?token=YOUR_TOKEN' \
  -H 'content-type: text/plain' \
  --data-binary @proxy.env
```
</Step>
<Step>
### Restart and verify persistence

Restart `anyllm_proxy --webui`, then open the Settings page or export the env file from the UI. Imported values are applied during startup after `.anyllm.env` discovery and before the async runtime begins serving requests.
</Step>
</Steps>

## What Happens Internally

`crates/proxy/src/env_parser.rs` provides `parse_env_content`, which is intentionally pure and side-effect free. The admin import endpoint can therefore parse content, collect `warnings`, reject `hard_errors`, and only persist the accepted key-value pairs if the content is safe. The pure parser also explains why the same env-file behavior is reused for CLI startup and admin imports without two separate parsers drifting apart.

## Safe Usage Pattern

- Keep deployment-critical secrets in your shell, process manager, or `.anyllm.env`.
- Use admin imports for controlled runtime changes, bootstrap flows, and team-visible config edits.
- Remember that env files still take precedence over imported values on the next startup.

One practical benefit of this design is recoverability. If the admin database is lost or cleared, the proxy can still boot from a normal env file, and if an imported value causes trouble, placing the corrected value in `.anyllm.env` will override it immediately on the next start. That is a small but important design choice from `main.rs`: imported values are treated as persisted operator input, not as a higher-priority source of truth than explicit deployment configuration.
