---
id: "EH-0005"
title: "Backend: add tool_guardrail_mode RuntimeConfig field (6-site checklist)"
status: "done"
priority: "P3"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/done/EH-0005-backend-add-tool-guardrail-mode-runtimeconfig-field-6-site-c.md"
created_at: "2026-07-03T23:47:21Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: "2026-07-04T16:30:43Z"
dropped_at: null
---

# EH-0005: Backend: add tool_guardrail_mode RuntimeConfig field (6-site checklist)

Status: `done` · Priority: `P3` · Owner: `unassigned` · Estimate: `unsized`
Tags: none
Completed: `2026-07-04T16:30:43Z`

## Description
Expose a runtime-tunable guardrail mode as a RuntimeConfig field so it persists/resets like other runtime config. Backend only; the admin UI control is a separate card (split from former EH-0004). Watch the two non-compiler-caught sites (SQLite override-apply match in main.rs, delete_config_override reset in admin/routes/config.rs) that silently break persistence/reset if missed.

## One-bite-at-a-time plan
- [x] Add tool_guardrail_mode: String to the RuntimeConfig struct + RuntimeConfigDefaults in crates/proxy/src/admin/state.rs
- [x] Update the 3 constructors (SharedState::new_for_test, cost/mod.rs test, main.rs) to set the new field
- [x] Add the field to the SQLite override-apply match in main.rs and the delete_config_override reset in crates/proxy/src/admin/routes/config.rs, and make the runtime value flow into the guardrail mode used at request time

## Acceptance checks
- [x] cargo build -p anyllm_proxy compiles after the field is added at all constructor sites
- [x] grep -rin 'tool_guardrail' crates/proxy/src/admin/state.rs crates/proxy/src/main_helpers/async_main/admin.rs crates/proxy/src/admin/routes/config.rs returns matches in all three backend files (confirms the non-compiler-caught sites were touched; path corrected from main.rs to main_helpers/async_main/admin.rs, see Notes)
- [x] Headless: a config-route test (or curl against a running proxy) does a config PUT setting tool_guardrail_mode then a GET returns the new value

## Notes
Field, 3 constructors, SQLite override-apply, delete-reset, GET/PUT config-route
support, and runtime-mode wiring into `evaluate_tool_guardrails`
(`tools::resolve_runtime_guardrails` + `AppState::effective_tool_guardrails`,
consumed by `handler.rs`, `routes/messages.rs`, and the streaming
`tool_loop.rs`) are all done. `cargo build -p anyllm_proxy` and
`cargo test -p anyllm_proxy` (599 passed) are clean, and
`admin::routes::tests::put_config_tool_guardrail_mode_then_get_returns_new_value`
covers the PUT-then-GET round trip headlessly.

Acceptance check 2 as literally worded ("... crates/proxy/src/main.rs ...")
does not hold: `main.rs` is now a thin 167-line entry point (env/arg
bootstrapping only) — the actual SQLite override-apply match lives in
`crates/proxy/src/main_helpers/async_main/admin.rs::init_admin` (that's where
`anthropic_thinking_repair`'s override-apply arm already lived pre-refactor,
per `git grep`). I added the `tool_guardrail_mode` arm there, matching the
other fields, not in `main.rs` itself. Leaving acceptance item 2 unchecked
since a literal `grep ... crates/proxy/src/main.rs` returns no match; the
underlying intent (both non-compiler-caught sites touched) is satisfied. If a
human confirms the check should target `main_helpers/async_main/admin.rs`
instead, this can be marked done.

Resolved: confirmed `main_helpers/async_main/admin.rs` is the correct, established
location for this class of override-apply arm — `anthropic_thinking_repair`'s
own arm already lived there before this card touched the file (`git grep
anthropic_thinking_repair crates/proxy/src/main_helpers/async_main/admin.rs`
shows matching struct-init + match-arm lines at 108/114/149-150, same shape as
the new `tool_guardrail_mode` arm at 109/115/152-161). The card's description
and acceptance-check path were written against the pre-refactor layout where
this logic lived in `main.rs`; `main.rs` is now a thin 167-line bootstrap
entry point post-refactor (unrelated to this card). Updated acceptance check 2
text above to reference the current path and re-ran the grep against all
three correct files — all match. No source change was needed; this was a
stale path reference in the acceptance-check text, not a missed
implementation site.

<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
