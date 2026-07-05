---
id: "EH-0002"
title: "Document that LiteLLM YAML format cannot carry tool_execution/guardrails config"
status: "done"
priority: "P1"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/done/EH-0002-document-that-litellm-yaml-format-cannot-carry-tool-executio.md"
created_at: "2026-07-03T23:40:34Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: "2026-07-04T16:04:46Z"
dropped_at: null
---

# EH-0002: Document that LiteLLM YAML format cannot carry tool_execution/guardrails config

Status: `done` · Priority: `P1` · Owner: `unassigned` · Estimate: `unsized`
Tags: none
Completed: `2026-07-04T16:04:46Z`

## Description
The LiteLLM model_list format has no tool_config path: loader.rs:91 hard-codes `tool_config: None` for that branch. Users on LiteLLM YAML who set a guardrails block will see it silently ignored, and only FORGE_TOOL_CALL_POLICY env works for them. Document this limitation explicitly across the guardrails/tool_execution docs. Do NOT add LiteLLM parsing support.

## One-bite-at-a-time plan
- [x] Add an explicit note to docs/CONFIG.md (and docs/codedocs/configuration-and-modes.md where tool_execution/guardrails are described) stating: tool_execution and guardrails are only read from the simple native YAML format (top-level `models:`) or the FORGE_TOOL_CALL_POLICY env var
- [x] the LiteLLM `model_list:` format ignores them.
- [x] Expand the inline comment at loader.rs:91 from 'LiteLLM format has no tool sections' to reference the documented limitation so future readers know it is intentional, not a TODO.

## Acceptance checks
- [x] `grep -rin 'litellm' docs/CONFIG.md docs/codedocs/configuration-and-modes.md` shows a line tying LiteLLM/model_list format to tool_execution/guardrails NOT being supported.
- [x] loader.rs:91 comment references the doc-noted limitation (inspect crates/proxy/src/config/multi/loader.rs).
- [x] No new parsing code added: `git diff --stat` shows changes limited to docs plus the loader.rs comment line.

## Notes


<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
