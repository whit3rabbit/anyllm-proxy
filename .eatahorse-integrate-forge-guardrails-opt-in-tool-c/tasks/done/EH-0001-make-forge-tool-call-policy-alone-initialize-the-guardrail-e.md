---
id: "EH-0001"
title: "Make FORGE_TOOL_CALL_POLICY alone initialize the guardrail engine (no YAML)"
status: "done"
priority: "P0"
owner: ""
tags: []
estimate: ""
source: "manual"
path: "tasks/done/EH-0001-make-forge-tool-call-policy-alone-initialize-the-guardrail-e.md"
created_at: "2026-07-03T23:40:25Z"
updated_at: "2026-07-04T16:39:24Z"
completed_at: "2026-07-04T15:59:55Z"
dropped_at: null
---

# EH-0001: Make FORGE_TOOL_CALL_POLICY alone initialize the guardrail engine (no YAML)

Status: `done` · Priority: `P0` · Owner: `unassigned` · Estimate: `unsized`
Tags: none
Completed: `2026-07-04T15:59:55Z`

## Description
The env-var-only local-LLM path is currently dead. init_tool_engine (crates/proxy/src/main_helpers/async_main/tools.rs:9) does `let tc = tool_config.filter(|tc| tc.has_any())?`; and returns None whenever there is no tool_execution/builtin/mcp section. But loader.rs sets tool_config: None when PROXY_CONFIG is unset (loader.rs:100-107) and has_any() is false for a simple YAML without a tool_execution block. So FORGE_TOOL_CALL_POLICY is never consulted in the exact scenario it targets. Fix the gating so a valid non-Disabled FORGE_TOOL_CALL_POLICY builds a ToolEngineState with an empty registry + default policy/loop_config, whether tool_config is None or present-but-empty. Preserve current behavior: unset/empty/invalid/disabled env with no YAML tool sections still yields None (or a Disabled engine that is inert).

## One-bite-at-a-time plan
- [x] Refactor init_tool_engine so the FORGE_TOOL_CALL_POLICY resolution runs BEFORE the has_any() early return: when tool_config is None or has_any() is false, resolve the env var and, if it parses to a non-Disabled ToolGuardrailMode, proceed to build the engine (empty registry, default policy/loop_config, no MCP)
- [x] otherwise return None as today. Keep the existing YAML-precedence rule (guardrails_set_in_yaml wins over env).
- [x] Add a #[cfg(test)] module (or extend the nearest tools test) covering: (a) tool_config None + FORGE_TOOL_CALL_POLICY=standard => Some engine with guardrails.mode != Disabled
- [x] (b) env unset + no tool sections => None
- [x] (c) invalid env value => None (or Disabled) and does not panic. Acquire crate::config::ENV_TEST_LOCK before mutating the env var and restore it after.

## Acceptance checks
- [x] New test passes: `cargo test -p anyllm_proxy forge_tool_call_policy_env_only` (or the chosen test name) exits 0.
- [x] Observable: with PROXY_CONFIG unset and FORGE_TOOL_CALL_POLICY=standard, init_tool_engine returns Some and the returned ToolEngineState.guardrails.mode is not ToolGuardrailMode::Disabled (asserted directly in the test).
- [x] `cargo clippy -p anyllm_proxy -- -D warnings` reports no warnings for the changed file.

## Notes


<!-- Eatahorse: edit checkboxes/status/title, then run `python scripts/eatahorse.py sync` or let the Claude Code hook sync it. -->
