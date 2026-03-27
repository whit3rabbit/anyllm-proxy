# Phase 2: Operator Value Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the proxy production-trustworthy for operators by wiring cost tracking, adding request attribution, creating an audit log, and adding spend threshold alerts. These close the gap between "features exist in code" and "features work end-to-end."

**Architecture:** Five independent tasks. Tasks 1-2 fix critical wiring gaps in existing code. Task 3 adds a new SQLite table. Task 4 adds webhook-based alerts. Task 5 adds a minimal policy middleware for model allowlists. No new crates or major structural changes.

**Tech Stack:** Rust stable (1.83+), axum 0.8, rusqlite 0.32, serde, tokio

---

## File Structure

No new files created except `crates/proxy/src/server/policy.rs` (Task 5). All other changes modify existing files.

| File | Responsibility |
|------|---------------|
| `crates/proxy/src/server/routes.rs` | Wire `record_cost()` calls into request handlers |
| `crates/proxy/src/server/streaming.rs` | Wire `record_cost()` into streaming completion |
| `crates/proxy/src/server/chat_completions.rs` | Wire `record_cost()` into chat completions |
| `crates/proxy/src/admin/db.rs` | Add `key_id` and `cost_usd` to request_log, add audit_log table |
| `crates/proxy/src/admin/routes.rs` | Emit audit events on key create/revoke/config change |
| `crates/proxy/src/cost/mod.rs` | Add spend alert threshold checking |
| `crates/proxy/src/server/policy.rs` | New: model allowlist/deny policy middleware |
| `crates/proxy/src/server/mod.rs` | Export policy module |

---

## Task 1: Wire Cost Tracking Into Request Handlers

**Problem:** `cost::record_cost()` exists, calculates cost from model pricing, and persists to SQLite, but it is **never called** from any request handler. Per-key `total_spend` and `period_spend_usd` are never incremented. Budget enforcement checks a value that never grows. This is a critical correctness bug.

**Files:**
- Modify: `crates/proxy/src/server/routes.rs` (non-streaming /v1/messages handler)
- Modify: `crates/proxy/src/server/streaming.rs` (streaming /v1/messages handler)
- Modify: `crates/proxy/src/server/chat_completions.rs` (both non-streaming and streaming)
- Modify: `crates/proxy/src/cost/mod.rs` (make `record_cost` accept the mapped model name)
- Test: Integration test in `crates/proxy/tests/virtual_keys.rs`

- [ ] **Step 1: Read the current `record_cost` signature and callers**

Read `crates/proxy/src/cost/mod.rs` to understand the `record_cost()` function signature, what it needs (SharedState, VirtualKeyContext, model name, input/output tokens), and confirm it is not called anywhere.

Run: `grep -rn "record_cost" crates/proxy/src/`
Expected: Only definition in `cost/mod.rs`, no callers.

- [ ] **Step 2: Read the non-streaming handler to find where token counts are available**

Read `crates/proxy/src/server/routes.rs`, specifically the `messages()` handler. Find where the response body is available and token counts can be extracted. The Anthropic response has `usage.input_tokens` and `usage.output_tokens`. The cost recording must happen AFTER the response is received but BEFORE returning to the client (or spawned async).

- [ ] **Step 3: Read the streaming handler to find where token counts are available**

Read `crates/proxy/src/server/streaming.rs`. In the spawned streaming task, after the stream completes, the `StreamingTranslator` should have accumulated token counts. Find where `log_request()` is called with token information, as that's the right place to also record cost.

- [ ] **Step 4: Read the chat completions handler for the same**

Read `crates/proxy/src/server/chat_completions.rs`. Both non-streaming and streaming paths need cost recording.

- [ ] **Step 5: Write a failing integration test**

In `crates/proxy/tests/virtual_keys.rs`, add a test that:
1. Creates a virtual key with a budget
2. Makes a request through the proxy
3. Queries the spend endpoint
4. Asserts `total_spend > 0.0`

This test will fail because `record_cost` is never called.

```rust
#[tokio::test]
async fn cost_accumulates_on_request() {
    // Create key, make request, check spend > 0
    // Use the existing test infrastructure from this file
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy cost_accumulates_on_request`
Expected: FAIL (spend is 0.0)

- [ ] **Step 7: Wire `record_cost` into non-streaming /v1/messages handler**

In `crates/proxy/src/server/routes.rs`, after the backend response is received and token counts are available, add:

```rust
// Record cost for virtual key spend tracking
if let Some(vk_ctx) = request.extensions().get::<VirtualKeyContext>() {
    let input_tokens = response_body.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let output_tokens = response_body.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0);
    crate::cost::record_cost(
        &state.shared,
        vk_ctx,
        &mapped_model,
        input_tokens,
        output_tokens,
    );
}
```

Adjust the exact field names and types based on what you find when reading the code. The key is: extract `input_tokens` and `output_tokens` from the response, and call `record_cost` with the `VirtualKeyContext` from request extensions.

- [ ] **Step 8: Wire `record_cost` into streaming /v1/messages handler**

In `crates/proxy/src/server/streaming.rs`, inside the spawned task after the stream completes (near where `log_request()` is called), extract the final token counts from the translator and call `record_cost`.

The streaming translator accumulates usage across all chunks. After the stream loop ends, get the final usage and record cost.

- [ ] **Step 9: Wire `record_cost` into chat completions handler (both paths)**

In `crates/proxy/src/server/chat_completions.rs`:
- Non-streaming: after receiving the response body, extract usage and call `record_cost`
- Streaming: in the spawned task after stream completion, extract accumulated usage and call `record_cost`

- [ ] **Step 10: Run tests**

Run: `cargo test -p anyllm_proxy cost_accumulates`
Expected: PASS

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass.

- [ ] **Step 11: Commit**

```bash
git add crates/proxy/src/server/routes.rs crates/proxy/src/server/streaming.rs crates/proxy/src/server/chat_completions.rs crates/proxy/src/cost/mod.rs crates/proxy/tests/virtual_keys.rs
git commit -m "fix: wire record_cost into all request handlers

record_cost() existed but was never called. Per-key total_spend and
period_spend_usd were never incremented, making budget enforcement
check a value that never grows. Now called after every request
(streaming and non-streaming) for both /v1/messages and
/v1/chat/completions endpoints."
```

---

## Task 2: Add Request Attribution (key_id + cost_usd to request_log)

**Problem:** The `request_log` table has no `key_id` column, so operators cannot trace requests back to specific virtual keys. It also has no `cost_usd` column, so per-request cost is not auditable.

**Files:**
- Modify: `crates/proxy/src/admin/db.rs` (alter schema, update insert/query)
- Modify: `crates/proxy/src/admin/state.rs` (add fields to RequestLogEntry)
- Modify: `crates/proxy/src/server/routes.rs` (pass key_id into log entry)
- Modify: `crates/proxy/src/server/streaming.rs` (pass key_id into log entry)
- Modify: `crates/proxy/src/server/chat_completions.rs` (pass key_id into log entry)
- Modify: `crates/proxy/src/admin/routes.rs` (expose key_id in GET /admin/api/requests response)
- Test: `crates/proxy/src/admin/db.rs` inline tests

- [ ] **Step 1: Read the current request_log schema and RequestLogEntry struct**

Read `crates/proxy/src/admin/db.rs` to find the CREATE TABLE statement and the insert function.
Read `crates/proxy/src/admin/state.rs` to find the `RequestLogEntry` struct and `AdminEvent` enum.

- [ ] **Step 2: Write a test for the new columns**

Add a test in `crates/proxy/src/admin/db.rs` that inserts a request log entry with `key_id` and `cost_usd` and verifies they can be queried back.

- [ ] **Step 3: Run test to verify it fails**

Expected: FAIL because the columns don't exist yet.

- [ ] **Step 4: Add columns to the schema**

In `crates/proxy/src/admin/db.rs`, update the `CREATE TABLE request_log` statement:

```sql
key_id          INTEGER,                    -- Virtual key ID (NULL for static/env keys)
cost_usd        REAL                        -- Calculated cost for this request
```

Also add a migration path: after the CREATE TABLE, add:
```sql
-- Migration: add columns if they don't exist (idempotent)
ALTER TABLE request_log ADD COLUMN key_id INTEGER;
ALTER TABLE request_log ADD COLUMN cost_usd REAL;
```

Wrap in a try/catch since SQLite doesn't have IF NOT EXISTS for ALTER TABLE. Use the pattern: attempt ALTER, ignore "duplicate column" error.

- [ ] **Step 5: Update RequestLogEntry struct**

In `crates/proxy/src/admin/state.rs`, add to `RequestLogEntry`:

```rust
pub key_id: Option<i64>,
pub cost_usd: Option<f64>,
```

- [ ] **Step 6: Update insert and query functions**

In `crates/proxy/src/admin/db.rs`:
- Update the INSERT statement to include `key_id` and `cost_usd`
- Update the SELECT/query functions to read the new columns
- Update the admin API response serialization

- [ ] **Step 7: Pass key_id from VirtualKeyContext into log entries**

In the request handlers (`routes.rs`, `streaming.rs`, `chat_completions.rs`), when constructing `RequestLogEntry` for `log_request()`, extract `key_id` from `VirtualKeyContext` if present:

```rust
let key_id = request.extensions().get::<VirtualKeyContext>().map(|ctx| ctx.key_id);
```

Set `cost_usd` from the cost calculation result (Task 1 should have made this available).

- [ ] **Step 8: Run tests**

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git commit -m "feat: add key_id and cost_usd to request_log for attribution

Operators can now trace requests to specific virtual keys and see
per-request cost. Enables filtered request logs by key and itemized
cost auditing."
```

---

## Task 3: Add Audit Log Table and Event Recording

**Problem:** No audit trail for admin actions. Key creation, revocation, config changes, and model additions/removals are not recorded. Operators cannot answer "who changed what and when."

**Files:**
- Modify: `crates/proxy/src/admin/db.rs` (add audit_log table, insert function)
- Modify: `crates/proxy/src/admin/routes.rs` (emit audit events after mutations)
- Modify: `crates/proxy/src/admin/state.rs` (add AuditEntry type)
- Test: `crates/proxy/src/admin/db.rs` inline tests

- [ ] **Step 1: Design the audit_log schema**

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,           -- ISO 8601 UTC
    action      TEXT NOT NULL,           -- 'key_created', 'key_revoked', 'config_changed', 'model_added', 'model_removed'
    target_type TEXT NOT NULL,           -- 'virtual_key', 'config', 'model'
    target_id   TEXT,                    -- Key ID, config key name, or model name
    detail      TEXT,                    -- JSON blob with action-specific details
    source_ip   TEXT                     -- Admin client IP (from request)
);
```

- [ ] **Step 2: Write test for audit log insert and query**

```rust
#[test]
fn audit_log_insert_and_query() {
    let conn = Connection::open_in_memory().unwrap();
    init_tables(&conn).unwrap();

    insert_audit_entry(&conn, &AuditEntry {
        action: "key_created".to_string(),
        target_type: "virtual_key".to_string(),
        target_id: Some("42".to_string()),
        detail: Some(r#"{"description":"test key"}"#.to_string()),
        source_ip: Some("127.0.0.1".to_string()),
    }).unwrap();

    let entries = query_audit_log(&conn, 10, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "key_created");
}
```

- [ ] **Step 3: Run test to verify it fails**

Expected: FAIL (no audit_log table or functions exist).

- [ ] **Step 4: Implement the schema and functions**

In `crates/proxy/src/admin/db.rs`:
- Add CREATE TABLE audit_log to `init_tables()`
- Add `insert_audit_entry()` function
- Add `query_audit_log()` function with limit/offset
- Add `AuditEntry` struct

- [ ] **Step 5: Add GET /admin/api/audit endpoint**

In `crates/proxy/src/admin/routes.rs`, add a new route:

```rust
.route("/admin/api/audit", get(get_audit_log))
```

Handler queries the audit_log table with optional limit/offset query params.

- [ ] **Step 6: Emit audit events in existing admin routes**

In `crates/proxy/src/admin/routes.rs`, add audit log writes after each mutation:

- `POST /admin/api/keys` (key creation): log `key_created` with description and key_prefix
- `DELETE /admin/api/keys/{id}` (key revocation): log `key_revoked` with key_id
- `PUT /admin/api/config` (config update): log `config_changed` with key and old/new values
- `DELETE /admin/api/config/overrides/{key}` (config delete): log `config_deleted` with key
- `POST /admin/api/models` (model add): log `model_added` with model name
- `DELETE /admin/api/models/{name}` (model remove): log `model_removed` with model name

Each audit write should be fire-and-forget (spawn_blocking) to avoid slowing the admin response.

- [ ] **Step 7: Run tests**

Run: `cargo test -p anyllm_proxy audit_log`
Expected: PASS

Run: `cargo test -p anyllm_proxy`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git commit -m "feat: add audit log for admin actions

New audit_log table records key_created, key_revoked, config_changed,
model_added, model_removed events with timestamp, target, detail, and
source IP. Queryable via GET /admin/api/audit."
```

---

## Task 4: Add Spend Threshold Alerts via Webhooks

**Problem:** No alerting when keys approach or exceed budget limits. Operators only discover overspend by manually checking the admin API. The proxy already has a webhook callback system (`callbacks.rs`) that fires on request completion.

**Files:**
- Modify: `crates/proxy/src/cost/mod.rs` (add threshold check after spend accumulation)
- Modify: `crates/proxy/src/callbacks.rs` (add spend alert event type, or use existing webhook)
- Test: `crates/proxy/src/cost/mod.rs` inline tests

- [ ] **Step 1: Read the existing callbacks system**

Read `crates/proxy/src/callbacks.rs` to understand how webhooks work (URL list, fire-and-forget POST, payload format).

- [ ] **Step 2: Design the alert thresholds**

Alerts fire when `period_spend_usd` crosses percentage thresholds of `max_budget_usd`:
- 80% -- warning
- 95% -- critical
- 100% -- exceeded (already enforced by middleware, but alert is also useful)

To avoid duplicate alerts per period, track the highest threshold crossed per key. Use an in-memory `DashMap<i64, u8>` keyed by `key_id` where value is the last alert level (0=none, 1=80%, 2=95%, 3=100%). Reset on period rollover.

- [ ] **Step 3: Write test for threshold detection**

```rust
#[test]
fn spend_threshold_detection() {
    assert_eq!(spend_threshold_level(80.0, 100.0), 1);  // 80%
    assert_eq!(spend_threshold_level(95.0, 100.0), 2);  // 95%
    assert_eq!(spend_threshold_level(100.0, 100.0), 3); // 100%
    assert_eq!(spend_threshold_level(79.9, 100.0), 0);  // below 80%
    assert_eq!(spend_threshold_level(50.0, 100.0), 0);  // well below
}
```

- [ ] **Step 4: Run test to verify it fails**

Expected: FAIL (function doesn't exist).

- [ ] **Step 5: Implement threshold detection and alerting**

In `crates/proxy/src/cost/mod.rs`, add:

```rust
/// Returns the spend alert level: 0=none, 1=80%, 2=95%, 3=100%
pub fn spend_threshold_level(spend: f64, budget: f64) -> u8 {
    if budget <= 0.0 {
        return 0;
    }
    let pct = spend / budget * 100.0;
    if pct >= 100.0 { 3 }
    else if pct >= 95.0 { 2 }
    else if pct >= 80.0 { 1 }
    else { 0 }
}
```

Add a `DashMap<i64, u8>` for tracking alerted levels. After `accumulate_spend()` completes, check the threshold and fire webhook if it's higher than the last alerted level.

The webhook payload should be:
```json
{
    "type": "spend_alert",
    "key_id": 42,
    "key_prefix": "sk-vk...",
    "threshold_pct": 80,
    "period_spend_usd": 80.50,
    "max_budget_usd": 100.00,
    "budget_duration": "monthly"
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p anyllm_proxy spend_threshold`
Expected: PASS

Run: `cargo test -p anyllm_proxy`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git commit -m "feat: spend threshold alerts at 80%/95%/100% via webhooks

When a virtual key's period_spend_usd crosses 80%, 95%, or 100% of
max_budget_usd, a webhook POST is fired with spend_alert payload.
Deduplication prevents repeat alerts within the same budget period."
```

---

## Task 5: Add Model Allowlist Policy Middleware

**Problem:** No mechanism for operators to restrict which models a virtual key can access. Any key can use any model. LiteLLM has policy controls; this is the minimum viable equivalent.

**Files:**
- Create: `crates/proxy/src/server/policy.rs` (new module)
- Modify: `crates/proxy/src/server/mod.rs` (export policy module)
- Modify: `crates/proxy/src/admin/keys.rs` (add `allowed_models` field to VirtualKeyMeta)
- Modify: `crates/proxy/src/admin/db.rs` (add `allowed_models` column)
- Modify: `crates/proxy/src/server/middleware.rs` (call policy check after auth)
- Modify: `crates/proxy/src/admin/routes.rs` (accept `allowed_models` in key creation)
- Test: `crates/proxy/src/server/policy.rs` inline tests

- [ ] **Step 1: Design the policy**

A virtual key can optionally have `allowed_models: Option<Vec<String>>`. When set, the proxy rejects requests for models not in the list. When None, all models are allowed (current behavior).

The list supports:
- Exact match: `"claude-sonnet-4-20250514"`
- Prefix match: `"claude-*"` (wildcard suffix)
- All: `None` or `["*"]`

- [ ] **Step 2: Write test for model policy check**

In `crates/proxy/src/server/policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_allowed_when_no_policy() {
        assert!(is_model_allowed("gpt-4o", &None));
    }

    #[test]
    fn model_allowed_exact_match() {
        let policy = Some(vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]);
        assert!(is_model_allowed("gpt-4o", &policy));
        assert!(is_model_allowed("gpt-4o-mini", &policy));
        assert!(!is_model_allowed("claude-sonnet-4-20250514", &policy));
    }

    #[test]
    fn model_allowed_wildcard() {
        let policy = Some(vec!["claude-*".to_string()]);
        assert!(is_model_allowed("claude-sonnet-4-20250514", &policy));
        assert!(is_model_allowed("claude-3-haiku", &policy));
        assert!(!is_model_allowed("gpt-4o", &policy));
    }

    #[test]
    fn model_allowed_star_allows_all() {
        let policy = Some(vec!["*".to_string()]);
        assert!(is_model_allowed("anything", &policy));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Expected: FAIL (module and function don't exist).

- [ ] **Step 4: Implement policy module**

Create `crates/proxy/src/server/policy.rs`:

```rust
//! Request policy enforcement.
//!
//! Policies are per-key restrictions that limit which models and
//! features a virtual key can access.

/// Check if a model name is allowed by the key's policy.
/// Returns true if no policy is set (all models allowed).
pub fn is_model_allowed(model: &str, allowed_models: &Option<Vec<String>>) -> bool {
    let Some(allowed) = allowed_models else {
        return true;
    };
    for pattern in allowed {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            if model.starts_with(prefix) {
                return true;
            }
        } else if pattern == model {
            return true;
        }
    }
    false
}
```

Add `pub mod policy;` to `crates/proxy/src/server/mod.rs`.

- [ ] **Step 5: Add `allowed_models` to VirtualKeyMeta**

In `crates/proxy/src/admin/keys.rs`, add to `VirtualKeyMeta`:

```rust
pub allowed_models: Option<Vec<String>>,
```

In `crates/proxy/src/admin/db.rs`:
- Add column: `allowed_models TEXT` (JSON-encoded list, nullable)
- Add migration: `ALTER TABLE virtual_api_key ADD COLUMN allowed_models TEXT;`
- Update insert/load to serialize/deserialize as JSON

- [ ] **Step 6: Enforce policy in middleware after model extraction**

In `crates/proxy/src/server/middleware.rs` or in the route handlers, after the model is extracted from the request body and after virtual key auth succeeds, check the policy:

```rust
if let Some(vk_ctx) = request.extensions().get::<VirtualKeyContext>() {
    // The allowed_models is on VirtualKeyMeta, accessible via the DashMap
    // Check model against policy
    if !policy::is_model_allowed(&model, &meta.allowed_models) {
        return Err(/* 403 with model not allowed error */);
    }
}
```

The exact placement depends on where the model name is available. It may need to go in the route handlers rather than the auth middleware, since the model is in the request body (parsed after auth).

- [ ] **Step 7: Accept `allowed_models` in key creation API**

In `crates/proxy/src/admin/routes.rs`, update the POST /admin/api/keys handler to accept:

```json
{
    "description": "restricted key",
    "allowed_models": ["claude-*", "gpt-4o"]
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p anyllm_proxy model_allowed`
Expected: All policy tests pass.

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git commit -m "feat: per-key model allowlist policy

Virtual keys can now restrict which models they can access via
allowed_models field (exact match and prefix wildcard). Enforced
before the request reaches the backend. Keys without a policy
continue to access all models."
```

---

## Summary

| Task | What | Files | Effort |
|------|------|-------|--------|
| 1 | Wire cost tracking into all handlers | routes.rs, streaming.rs, chat_completions.rs, cost/mod.rs | M |
| 2 | Request attribution (key_id + cost in request_log) | admin/db.rs, admin/state.rs, routes.rs, streaming.rs | M |
| 3 | Audit log table + event recording | admin/db.rs, admin/routes.rs, admin/state.rs | M |
| 4 | Spend threshold alerts via webhooks | cost/mod.rs, callbacks.rs | S |
| 5 | Model allowlist policy per key | server/policy.rs, admin/keys.rs, admin/db.rs, middleware.rs | M |

After completing all 5 tasks, run the full verification:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

All must pass with zero warnings before considering Phase 2 complete.
