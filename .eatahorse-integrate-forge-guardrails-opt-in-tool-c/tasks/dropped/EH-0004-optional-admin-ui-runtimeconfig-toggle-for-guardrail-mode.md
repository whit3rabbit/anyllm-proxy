---
id: "EH-0004"
title: "(Optional) Admin UI + RuntimeConfig toggle for guardrail mode"
status: "dropped"
priority: "P3"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/dropped/EH-0004-optional-admin-ui-runtimeconfig-toggle-for-guardrail-mode.md"
created_at: "2026-07-03T23:40:55Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: null
dropped_at: "2026-07-03T23:47:21Z"
---

# EH-0004: (Optional) Admin UI + RuntimeConfig toggle for guardrail mode

Status: `dropped` · Priority: `P3` · Owner: `unassigned` · Estimate: `unsized`
Tags: none

## Description
No admin surface exists for tool_execution guardrails (admin-ui/src has zero references). Optionally expose a runtime-tunable guardrail mode following the documented 6-site RuntimeConfig-field checklist so it persists and resets like other runtime config. Lower priority than the P0/P1 cards; skip if scope is tight. Note the two non-compiler-caught sites (SQLite override-apply match in main.rs, delete_config_override reset in admin/routes/config.rs) that silently break persistence/reset if missed.

## One-bite-at-a-time plan
- [ ] Add a RuntimeConfig field (e.g. tool_guardrail_mode: String) at all 6 sites: struct + RuntimeConfigDefaults in crates/proxy/src/admin/state.rs, the 3 constructors (SharedState::new_for_test, cost/mod.rs test, main.rs), the SQLite override-apply match in main.rs, and delete_config_override reset in crates/proxy/src/admin/routes/config.rs. Make the runtime value flow into the guardrail mode actually used at request time.
- [ ] Add a toggle/select control to crates/proxy/admin-ui/src/tabs/settings/Settings.tsx wired to the existing admin config GET/PUT route for the new field.

## Acceptance checks
- [ ] `cargo build -p anyllm_proxy` compiles after the field is added at all constructor sites.
- [ ] `grep -rin 'tool_guardrail' crates/proxy/src/admin/state.rs crates/proxy/src/main.rs crates/proxy/src/admin/routes/config.rs crates/proxy/admin-ui/src/tabs/settings/Settings.tsx` returns matches in all four (confirming all non-compiler-caught sites were touched).
- [ ] Headless: an admin config PUT setting the field then GET returns the new value (assert via a config-route test or curl against a running proxy). (manual residual: toggle the control in `cargo run -p anyllm_proxy -- --webui` and confirm it survives a restart.)

## Notes


<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
