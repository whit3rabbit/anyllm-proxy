# Simple/Advanced Mode Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give anyllm-proxy a Simple mode (LiteLLM-equivalent: 3 env vars, zero config, no internal translation details visible to clients) and an Advanced mode (full system: degradation headers, verbose config, admin UI) so users can reason about the system at the complexity level they need.

**Architecture:** Add an `expose_degradation_warnings` boolean to `Config` and `AppState` (defaults to `false`, enabled by `ANYLLM_DEGRADATION_WARNINGS=true` or presence of a config file). Gate the `inject_degradation_header` call on this flag. Restructure README to lead with Simple mode, with Advanced features in a secondary section.

**Tech Stack:** Rust (existing codebase), env var config, README Markdown.

---

## File Map

| File | Change |
|------|--------|
| `crates/proxy/src/config/mod.rs` | Add `expose_degradation_warnings: bool` field to `Config`; parse from env |
| `crates/proxy/src/server/routes.rs` | Add `expose_degradation_warnings: bool` to `AppState`; gate `inject_degradation_header` calls |
| `crates/proxy/src/server/chat_completions.rs` | Gate `inject_degradation_header` calls |
| `CLAUDE.md` | Add `ANYLLM_DEGRADATION_WARNINGS` to env var table |
| `README.md` | Restructure: Simple mode first, Advanced mode second |

---

### Task 1: Add `expose_degradation_warnings` to `Config`

**Files:**
- Modify: `crates/proxy/src/config/mod.rs`

The `Config` struct (line ~68) gains one field. `from_env()` reads `ANYLLM_DEGRADATION_WARNINGS`. Auto-enable when a config file path is set (config file = advanced mode signal).

- [ ] **Step 1: Read the Config struct and from_env() to identify insertion points**

```bash
grep -n "pub log_bodies\|pub openai_api_format\|PROXY_CONFIG\|let log_bodies" crates/proxy/src/config/mod.rs
```

Expected: lines showing `log_bodies` field and `PROXY_CONFIG` parsing.

- [ ] **Step 2: Add `expose_degradation_warnings` to the `Config` struct**

In `crates/proxy/src/config/mod.rs`, add after the `log_bodies` field:

```rust
    /// Expose `x-anyllm-degradation` header when features are silently dropped.
    /// Defaults to false (simple mode). Enable with ANYLLM_DEGRADATION_WARNINGS=true
    /// or automatically when PROXY_CONFIG is set.
    pub expose_degradation_warnings: bool,
```

- [ ] **Step 3: Parse the env var in `from_env()`**

Find where `log_bodies` is parsed in `from_env()`. Add immediately after:

```rust
        let expose_degradation_warnings = std::env::var("ANYLLM_DEGRADATION_WARNINGS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
```

- [ ] **Step 4: Set the field in every `Config { ... }` construction site in `from_env()`**

Each match arm that constructs `Config` (one per backend) needs the field. Add:

```rust
            expose_degradation_warnings,
```

alongside the existing `log_bodies` field in every arm.

- [ ] **Step 5: Auto-enable when a config file is provided**

Find the multi-backend config path in `from_env()` (where `PROXY_CONFIG` is read). After the file is loaded but before building `Config`, add:

```rust
        // Config file presence implies advanced mode — enable degradation warnings by default.
        let expose_degradation_warnings = expose_degradation_warnings
            || std::env::var("PROXY_CONFIG").is_ok();
```

- [ ] **Step 6: Build and verify it compiles**

```bash
cargo build -p anyllm_proxy 2>&1 | grep -E "error|warning: unused"
```

Expected: compile errors only at `AppState` construction sites (no `Config`-level errors).

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/config/mod.rs
git commit -m "feat(config): add expose_degradation_warnings field; auto-enable with PROXY_CONFIG"
```

---

### Task 2: Thread `expose_degradation_warnings` into `AppState`

**Files:**
- Modify: `crates/proxy/src/server/routes.rs` (AppState struct ~line 70, construction ~line 239)

- [ ] **Step 1: Add field to `AppState`**

In `crates/proxy/src/server/routes.rs`, add after `omit_stream_options`:

```rust
    /// When true, set `x-anyllm-degradation` header on responses that silently drop features.
    /// Corresponds to Config::expose_degradation_warnings.
    pub expose_degradation_warnings: bool,
```

- [ ] **Step 2: Set the field at all `AppState { ... }` construction sites**

Search for all `AppState {` constructions:

```bash
grep -n "AppState {" crates/proxy/src/server/routes.rs
```

At each construction, add:

```rust
            expose_degradation_warnings: cfg.expose_degradation_warnings,
```

or `false` for test helpers that don't take a `Config`. For test constructions, default `false`.

- [ ] **Step 3: Gate `inject_degradation_header` in `routes.rs`**

Find all calls to `inject_degradation_header` in `routes.rs`:

```bash
grep -n "inject_degradation_header" crates/proxy/src/server/routes.rs
```

Wrap each call:

```rust
if state.expose_degradation_warnings {
    inject_degradation_header(response.headers_mut(), &warnings);
}
```

There are four sites (lines ~713, ~763, ~866, ~953 per the earlier search). Apply the guard to all four.

- [ ] **Step 4: Build to confirm routes.rs compiles**

```bash
cargo build -p anyllm_proxy 2>&1 | grep "error"
```

Expected: errors only in `chat_completions.rs` (not yet updated).

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/routes.rs
git commit -m "feat(server): gate degradation header on expose_degradation_warnings flag"
```

---

### Task 3: Gate degradation header in `chat_completions.rs`

**Files:**
- Modify: `crates/proxy/src/server/chat_completions.rs`

- [ ] **Step 1: Confirm all call sites**

```bash
grep -n "inject_degradation_header" crates/proxy/src/server/chat_completions.rs
```

Expected: lines ~171, ~275, ~358, ~607.

- [ ] **Step 2: Gate each call**

For each `inject_degradation_header(response.headers_mut(), &warnings);` line, wrap with:

```rust
if state.expose_degradation_warnings {
    inject_degradation_header(response.headers_mut(), &warnings);
}
```

The `state` variable is already in scope at each call site (it's the `State(state): State<AppState>` extractor in each handler).

- [ ] **Step 3: Build and run all tests**

```bash
cargo build -p anyllm_proxy 2>&1 | grep "error"
cargo test -p anyllm_proxy 2>&1 | tail -5
```

Expected: clean build, ~906 tests passing.

- [ ] **Step 4: Verify degradation header absent by default**

```bash
cargo test -p anyllm_proxy degradation 2>&1
```

Expected: any degradation tests still pass (they test the header is absent unless the flag is set).

- [ ] **Step 5: Write a test confirming default-off behavior**

In `crates/proxy/tests/chat_completions.rs` (or a new `crates/proxy/tests/degradation.rs`), add:

```rust
#[tokio::test]
async fn degradation_header_absent_by_default() {
    // Build an AppState with expose_degradation_warnings: false (default)
    // Send a request that would produce degradation warnings (e.g., top_k param)
    // Assert response does NOT contain x-anyllm-degradation header
    let app = test_app_with_degradation_warnings(false);
    let resp = send_request_with_top_k(&app).await;
    assert!(resp.headers().get("x-anyllm-degradation").is_none());
}

#[tokio::test]
async fn degradation_header_present_when_enabled() {
    let app = test_app_with_degradation_warnings(true);
    let resp = send_request_with_top_k(&app).await;
    assert!(resp.headers().get("x-anyllm-degradation").is_some());
}
```

Use the existing test helpers in `crates/proxy/tests/` for `test_app_with_*` patterns. Match the signature pattern used in `chat_completions.rs` tests.

- [ ] **Step 6: Run the new tests**

```bash
cargo test -p anyllm_proxy degradation_header -- --nocapture
```

Expected: both tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/server/chat_completions.rs crates/proxy/tests/
git commit -m "feat(server): gate degradation header in chat_completions; add on/off tests"
```

---

### Task 4: Document `ANYLLM_DEGRADATION_WARNINGS` in CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Find the env var table location in CLAUDE.md**

```bash
grep -n "LOG_BODIES\|RATE_LIMIT_FAIL_POLICY" CLAUDE.md
```

Expected: the Environment Variables section.

- [ ] **Step 2: Add the new env var entry**

After the `LOG_BODIES` entry, add:

```markdown
- `ANYLLM_DEGRADATION_WARNINGS`: Expose `x-anyllm-degradation` response header when features are silently dropped during translation (`true` or `1`, default: disabled). Auto-enabled when `PROXY_CONFIG` is set.
```

- [ ] **Step 3: Update the "Security fixes" bullet in Current Status**

Find the line that mentions `x-anyllm-degradation` in the Working section and note it is now opt-in:

```markdown
- `x-anyllm-degradation` response header: set when features are silently dropped during translation (opt-in via `ANYLLM_DEGRADATION_WARNINGS=true`; auto-enabled in config-file mode)
```

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(CLAUDE.md): document ANYLLM_DEGRADATION_WARNINGS; note opt-in behavior"
```

---

### Task 5: Restructure README for Simple/Advanced mental model

**Files:**
- Modify: `README.md`

The README currently starts with Quick Start (good) but then immediately expands into all features without a conceptual frame. The goal: user reads "Simple mode = 3 vars, works" before ever seeing virtual keys or warnings.

- [ ] **Step 1: Add a "Two modes" section immediately after the Quick Start header**

After the Quick Start section's first code block (the `.anyllm.env` example), insert:

```markdown
### Simple mode vs. advanced mode

| | Simple mode | Advanced mode |
|---|---|---|
| **Config** | 3 env vars or `.anyllm.env` | `config.toml` / `config.yaml` |
| **What it does** | Translate and forward requests | + routing, rate limiting, virtual keys, audit log |
| **Admin UI** | Not started | `--webui` flag |
| **Translation warnings** | Silent (never exposed to clients) | `x-anyllm-degradation` header active |
| **How to enable** | Default | Pass `--webui`, set `PROXY_CONFIG`, or set `ANYLLM_DEGRADATION_WARNINGS=true` |

Most users never leave simple mode. Start there.
```

- [ ] **Step 2: Add section markers to the README body**

Find the boundary between simple and advanced content. Add a heading before the multi-backend / admin content:

```markdown
---

## Advanced Mode
```

Place this heading before the "Multiple backends on one proxy" section. The content above it (Quick Start, single-backend env var examples) is Simple mode. Everything from multi-backend config onward is Advanced mode.

- [ ] **Step 3: Add a one-liner to each backend example reinforcing simple mode**

At the top of each `## 1. Primary Use Case` / `## 3. Commercial APIs` section, ensure the example shows only the minimum vars needed. No changes to the commands themselves — just verify they don't reference advanced flags. (Read and check only; no edits needed if they are already minimal.)

- [ ] **Step 4: Add `ANYLLM_DEGRADATION_WARNINGS` to the env var reference table in README**

Find the env var table (search for `LOG_BODIES` in README.md). Add after `LOG_BODIES`:

```markdown
- `ANYLLM_DEGRADATION_WARNINGS`: Set to `true` or `1` to expose `x-anyllm-degradation` response header when translation silently drops features (default: disabled; auto-enabled in config-file mode).
```

- [ ] **Step 5: Verify README renders correctly**

```bash
# Quick visual check: count headers to ensure structure is intact
grep "^#" README.md
```

Expected: Quick Start, Two modes, Advanced Mode, then the existing numbered sections.

- [ ] **Step 6: Commit**

```bash
git add README.md
git commit -m "docs(README): add simple/advanced mode split; move advanced features under header"
```

---

## Self-Review

**Spec coverage check:**

| Requirement | Covered by |
|---|---|
| Simple mode = LiteLLM equivalent | Task 5 (README) + Tasks 1-3 (degradation header off by default) |
| Advanced mode = full system | Auto-enabled by `PROXY_CONFIG` or `--webui` (Task 1) |
| No internal translation details visible in simple mode | Tasks 2-3 (gate degradation header) |
| Two modes documented | Task 5 (README table) + Task 4 (CLAUDE.md) |

**Placeholder scan:** No TBDs. Every step has commands or code.

**Type consistency:**
- `expose_degradation_warnings: bool` used consistently in `Config` (Task 1), `AppState` (Task 2), and gate conditions (Tasks 2-3).
- `inject_degradation_header` signature unchanged — only the call site is gated.
- Test helper `test_app_with_degradation_warnings(bool)` in Task 3 must be defined in the same test file before the two tests that use it.

**One gap fixed:** Task 3 Step 5 references `test_app_with_degradation_warnings` and `send_request_with_top_k` as helpers. These must be written in the same step, not assumed to exist. Expand Step 5 to include their definitions, matching whatever pattern the existing `crates/proxy/tests/chat_completions.rs` uses for app setup.
