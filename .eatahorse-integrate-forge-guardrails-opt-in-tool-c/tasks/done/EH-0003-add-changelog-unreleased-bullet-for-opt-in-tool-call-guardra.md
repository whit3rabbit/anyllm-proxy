---
id: "EH-0003"
title: "Add CHANGELOG [Unreleased] bullet for opt-in tool-call guardrails"
status: "done"
priority: "P2"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/done/EH-0003-add-changelog-unreleased-bullet-for-opt-in-tool-call-guardra.md"
created_at: "2026-07-03T23:40:45Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: "2026-07-04T16:07:46Z"
dropped_at: null
---

# EH-0003: Add CHANGELOG [Unreleased] bullet for opt-in tool-call guardrails

Status: `done` · Priority: `P2` · Owner: `unassigned` · Estimate: `unsized`
Tags: none
Completed: `2026-07-04T16:07:46Z`

## Description
Per the repo release process, every user-visible change needs an [Unreleased] bullet before release. The guardrails feature (config + FORGE_TOOL_CALL_POLICY env + nudges) is user-visible and the section is currently empty (CHANGELOG.md line ~11).

## One-bite-at-a-time plan
- [x] Under the `## [Unreleased]` header in CHANGELOG.md, add an `### Added` bullet describing opt-in Forge-style tool-call guardrails (lsp_first/quiet_command/write_payload_cap nudges, fingerprint dedup) configurable via simple YAML tool_execution.guardrails or the FORGE_TOOL_CALL_POLICY env var, noting the LiteLLM-format limitation.

## Acceptance checks
- [x] `grep -in 'guardrail' CHANGELOG.md` returns a line located between the `## [Unreleased]` and the next `## [` version header.
- [x] The bullet mentions both FORGE_TOOL_CALL_POLICY and the simple-YAML tool_execution.guardrails path.

## Notes


<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
