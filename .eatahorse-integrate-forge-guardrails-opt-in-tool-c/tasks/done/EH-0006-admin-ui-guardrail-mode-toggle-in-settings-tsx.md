---
id: "EH-0006"
title: "Admin UI: guardrail mode toggle in Settings.tsx"
status: "done"
priority: "P3"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/done/EH-0006-admin-ui-guardrail-mode-toggle-in-settings-tsx.md"
created_at: "2026-07-03T23:47:21Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: "2026-07-04T16:37:17Z"
dropped_at: null
---

# EH-0006: Admin UI: guardrail mode toggle in Settings.tsx

Status: `done` · Priority: `P3` · Owner: `unassigned` · Estimate: `unsized`
Tags: none
Completed: `2026-07-04T16:37:17Z`

## Description
Add a toggle/select control for tool_guardrail_mode to the admin UI, wired to the existing admin config GET/PUT route. Depends on the backend RuntimeConfig-field card being done first (split from former EH-0004).

## One-bite-at-a-time plan
- [x] Add a select/toggle control for tool_guardrail_mode to crates/proxy/admin-ui/src/tabs/settings/Settings.tsx, wired to the existing admin config GET/PUT route

## Acceptance checks
- [x] grep -rin 'tool_guardrail' crates/proxy/admin-ui/src/tabs/settings/Settings.tsx returns the new control
- [x] npm run build in crates/proxy/admin-ui compiles with the new control present
- [x] Manual: in cargo run -p anyllm_proxy -- --webui, toggling the control then restarting shows the value persisted

## Notes
Verified the "manual" acceptance item headlessly by driving the same admin API the UI control
calls (no browser available in this sandbox): started `cargo run -p anyllm_proxy -- --webui`
against a scratch `ANYLLM_HOME`, fetched the admin token + CSRF token, `PUT
/admin/api/config {"tool_guardrail_mode":"standard"}` (confirmed `disabled` -> `standard` and
`overridden_keys` includes it), killed and restarted the process, and confirmed
`GET /admin/api/config` still returned `tool_guardrail_mode: "standard"` with the override
intact. This exercises the exact backend path the new `<select>` in Settings.tsx invokes
(`save.mutate({ tool_guardrail_mode })` -> `PUT /admin/api/config`), so it stands in for a
literal browser click.

<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
