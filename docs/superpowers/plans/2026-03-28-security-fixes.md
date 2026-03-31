# Security Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix four confirmed security and logical bugs: X-Forwarded-For spoofing in IP allowlist, IPv6 SSRF gaps, budget period desync across restarts, and Langfuse timestamp truncation.

**Architecture:** Each fix is isolated to a single function or small set of functions with no cross-cutting concerns. All changes are covered by existing test infrastructure; new unit tests are added for each case. No new dependencies are added.

**Tech Stack:** Rust stable 1.83+, rusqlite, axum, tokio, existing test infra (`#[cfg(test)]` modules, in-memory SQLite via `rusqlite::Connection::open_in_memory`)

---

## File Map

| File | Change |
|------|--------|
| `crates/proxy/src/server/middleware.rs` | Take rightmost XFF IP; propagate period_reset via VirtualKeyContext |
| `crates/client/src/http.rs` | Add ULA + link-local IPv6 checks to `is_private_ip` |
| `crates/proxy/src/cost/db.rs` | Add `reset_period_spend` function |
| `crates/proxy/src/cost/mod.rs` | Call `reset_period_spend` before `accumulate_spend` when period rolled |
| `crates/proxy/src/integrations/langfuse.rs` | Fix `iso8601_to_epoch` to return milliseconds; update call sites |

---

### Task 1: Fix X-Forwarded-For Spoofing (High)

**Location:** `crates/proxy/src/server/middleware.rs:518-527`

**Root cause:** `s.split(',').next()` returns the *leftmost* IP, which is attacker-controlled. In a proxied setup, the reverse proxy *appends* the real client IP to the right. The rightmost IP is the one added by the trusted proxy.

**Files:**
- Modify: `crates/proxy/src/server/middleware.rs:518-527`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block at the bottom of `middleware.rs`:

```rust
#[test]
fn xff_spoofed_loopback_is_ignored() {
    // Simulate: attacker sends X-Forwarded-For: 127.0.0.1
    // Trusted proxy appends attacker's real IP: "127.0.0.1, 203.0.113.5"
    // We must resolve to the rightmost IP (203.0.113.5), NOT 127.0.0.1.
    let header_value = "127.0.0.1, 203.0.113.5";
    let resolved: std::net::IpAddr = header_value
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .last()
        .and_then(|s| s.parse().ok())
        .unwrap();
    assert_eq!(resolved, "203.0.113.5".parse::<std::net::IpAddr>().unwrap());
    assert_ne!(resolved, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());
}

#[test]
fn xff_single_ip_still_resolves() {
    let header_value = "10.0.1.5";
    let resolved: std::net::IpAddr = header_value
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .last()
        .and_then(|s| s.parse().ok())
        .unwrap();
    assert_eq!(resolved, "10.0.1.5".parse::<std::net::IpAddr>().unwrap());
}
```

- [ ] **Step 2: Run tests to confirm current behavior (both should pass because they test the extraction logic in isolation)**

```bash
cargo test -p anyllm_proxy xff -- --nocapture
```

Expected: both pass (the extraction logic is inline in tests, not calling middleware).

- [ ] **Step 3: Apply the fix in `check_ip_allowlist`**

In `crates/proxy/src/server/middleware.rs`, find lines 518-527:

```rust
    let client_ip = if *TRUST_PROXY_HEADERS {
        request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
    } else {
        None
    };
```

Replace with:

```rust
    let client_ip = if *TRUST_PROXY_HEADERS {
        // Take the *rightmost* IP: trusted proxy appends the real client IP.
        // Taking the leftmost allows spoofing via a crafted X-Forwarded-For header.
        request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(',')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .last()
            })
            .and_then(|s| s.parse::<std::net::IpAddr>().ok())
    } else {
        None
    };
```

- [ ] **Step 4: Run full proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: ~all passing, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/middleware.rs
git commit -m "fix(security): take rightmost X-Forwarded-For IP to prevent allowlist spoofing"
```

---

### Task 2: Fix IPv6 SSRF Bypass via ULA/Link-Local (High)

**Location:** `crates/client/src/http.rs:131-136`

**Root cause:** IPv6 Unique Local Addresses (`fc00::/7`) and Link-Local addresses (`fe80::/10`) are not checked. `Ipv6Addr::is_unique_local()` is unstable; the fix uses bitwise checks on the first 16-bit segment.

**Files:**
- Modify: `crates/client/src/http.rs:131-136`

- [ ] **Step 1: Write failing tests**

Add these tests to the existing `#[cfg(test)]` block in `crates/client/src/http.rs` (after the existing `public_ipv6` test):

```rust
#[test]
fn private_ipv6_ula_fc() {
    // fc00::/7 — Unique Local Address range (fc prefix)
    assert!(is_private_ip("fc00::1".parse().unwrap()));
    assert!(is_private_ip("fdff:ffff:ffff:ffff::1".parse().unwrap()));
}

#[test]
fn private_ipv6_ula_fd() {
    // fd00::/8 is within fc00::/7
    assert!(is_private_ip("fd12:3456:789a:1::1".parse().unwrap()));
}

#[test]
fn private_ipv6_link_local() {
    // fe80::/10 — Link-Local addresses
    assert!(is_private_ip("fe80::1".parse().unwrap()));
    assert!(is_private_ip("fe80::dead:beef".parse().unwrap()));
    assert!(is_private_ip("febf::1".parse().unwrap())); // fe80::/10 upper boundary
}

#[test]
fn public_ipv6_not_blocked() {
    // These are public and must NOT be blocked
    assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap())); // Google DNS
    assert!(!is_private_ip("2606:4700:4700::1111".parse().unwrap())); // Cloudflare
    assert!(!is_private_ip("ff02::1".parse().unwrap()));              // multicast, not ULA
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -p anyllm_client private_ipv6_ula private_ipv6_link_local -- --nocapture
```

Expected: FAIL — `fc00::1`, `fd12::1`, `fe80::1` currently return `false` (not blocked).

- [ ] **Step 3: Apply the fix**

In `crates/client/src/http.rs`, find lines 131-136:

```rust
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified()
            // Check IPv4-mapped IPv6 addresses (::ffff:192.168.x.x) recursively;
            // attackers can bypass IPv4 checks using the mapped representation.
            || matches!(v6.to_ipv4_mapped(), Some(v4) if is_private_ip(IpAddr::V4(v4)))
        }
```

Replace with:

```rust
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                // Unique Local Addresses (fc00::/7): covers fc00:: through fdff::
                || (seg0 & 0xfe00) == 0xfc00
                // Link-Local addresses (fe80::/10): covers fe80:: through febf::
                || (seg0 & 0xffc0) == 0xfe80
                // IPv4-mapped (::ffff:x.x.x.x): check recursively against IPv4 rules
                || matches!(v6.to_ipv4_mapped(), Some(v4) if is_private_ip(IpAddr::V4(v4)))
        }
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test -p anyllm_client
```

Expected: all passing including the new ULA and link-local tests.

- [ ] **Step 5: Commit**

```bash
git add crates/client/src/http.rs
git commit -m "fix(security): block IPv6 ULA (fc00::/7) and link-local (fe80::/10) in SSRF guard"
```

---

### Task 3: Fix Budget Period Desync Across Restarts (Medium)

**Location:** `crates/proxy/src/cost/db.rs`, `crates/proxy/src/cost/mod.rs`, `crates/proxy/src/server/middleware.rs`

**Root cause:**
1. `check_and_reset_period` resets `meta.period_spend_usd = 0.0` in the DashMap entry but never writes to SQLite.
2. `accumulate_spend` does `period_spend_usd = period_spend_usd + ?1` — SQL adds cost to the stale SQLite value.
3. On restart, the bloated SQLite `period_spend_usd` is loaded back into memory, falsely enforcing a spent-out budget.

**Fix sequence:**
- Add `reset_period_spend(conn, key_id, new_period_start)` to `cost/db.rs`.
- Add `period_reset: Option<String>` to `VirtualKeyContext` so the post-response `record_cost` knows to reset first.
- In `record_cost`, when `period_reset` is `Some`, call `reset_period_spend` before `accumulate_spend` in the same blocking task (so they run atomically under the mutex).

**Files:**
- Modify: `crates/proxy/src/cost/db.rs` (new function)
- Modify: `crates/proxy/src/server/middleware.rs` (VirtualKeyContext + populate on reset)
- Modify: `crates/proxy/src/cost/mod.rs` (call reset before accumulate)

- [ ] **Step 1: Add `reset_period_spend` to `cost/db.rs` and write the test**

In `crates/proxy/src/cost/db.rs`, add after the `accumulate_spend` function:

```rust
/// Atomically reset the period budget in SQLite when a new budget period begins.
/// Called when `check_and_reset_period` triggers a rollover; must run before
/// `accumulate_spend` so the running total starts from zero for the new period.
pub fn reset_period_spend(
    conn: &Connection,
    key_id: i64,
    new_period_start: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE virtual_api_key SET period_spend_usd = 0.0, period_start = ?1 WHERE id = ?2",
        params![new_period_start, key_id],
    )?;
    Ok(())
}
```

Add to the `#[cfg(test)]` block in `cost/db.rs`:

```rust
#[test]
fn reset_period_spend_zeroes_and_updates_start() {
    let conn = test_db();
    let id = crate::admin::db::insert_virtual_key(
        &conn,
        &crate::admin::db::InsertVirtualKeyParams {
            key_hash: "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd",
            key_prefix: "sk-vkaabb",
            description: Some("period-reset-test"),
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            spend_limit: None,
            role: "developer",
            max_budget_usd: Some(10.0),
            budget_duration: Some("monthly"),
            allowed_models: None,
        },
    )
    .unwrap();

    // Accumulate spend in the old period
    accumulate_spend(&conn, id, 7.50, 1000, 500).unwrap();
    let spend = get_key_spend(&conn, id).unwrap().unwrap();
    assert!((spend.period_cost_usd - 7.50).abs() < 1e-10);

    // Simulate period rollover
    reset_period_spend(&conn, id, "2026-04-01T00:00:00Z").unwrap();
    let spend = get_key_spend(&conn, id).unwrap().unwrap();
    assert!((spend.period_cost_usd - 0.0).abs() < 1e-10);
    assert_eq!(spend.period_start.as_deref(), Some("2026-04-01T00:00:00Z"));

    // Accumulate in the new period
    accumulate_spend(&conn, id, 1.25, 200, 100).unwrap();
    let spend = get_key_spend(&conn, id).unwrap().unwrap();
    // period_cost_usd must be 1.25 (not 7.50 + 1.25 = 8.75)
    assert!((spend.period_cost_usd - 1.25).abs() < 1e-10);
    // total_spend must be cumulative
    assert!((spend.total_cost_usd - 8.75).abs() < 1e-10);
}
```

- [ ] **Step 2: Run the new test to confirm it fails**

```bash
cargo test -p anyllm_proxy reset_period_spend -- --nocapture
```

Expected: FAIL — `reset_period_spend` does not exist yet. (If you added the function already, run just the assertion test to confirm the DB wiring is correct.)

- [ ] **Step 3: Add `period_reset` field to `VirtualKeyContext`**

In `crates/proxy/src/server/middleware.rs`, find the `VirtualKeyContext` struct (line ~45):

```rust
#[derive(Clone)]
pub struct VirtualKeyContext {
    pub(crate) key_id: i64,
    pub(crate) rate_state: Arc<RateLimitState>,
    pub(crate) allowed_models: Option<Vec<String>>,
}
```

Replace with:

```rust
#[derive(Clone)]
pub struct VirtualKeyContext {
    pub(crate) key_id: i64,
    pub(crate) rate_state: Arc<RateLimitState>,
    pub(crate) allowed_models: Option<Vec<String>>,
    /// Set to the new period_start ISO string when a budget period was reset
    /// during this request's auth check. Signals `record_cost` to call
    /// `reset_period_spend` before `accumulate_spend` so SQLite stays in sync.
    pub(crate) period_reset: Option<String>,
}
```

- [ ] **Step 4: Populate `period_reset` in the auth middleware**

In `middleware.rs`, find the block around lines 341-377 where `check_and_reset_period` is called and `VirtualKeyContext` is inserted. Update the insert to include the new field:

Find:
```rust
            // Budget enforcement: lazy period reset then check
            if meta.max_budget_usd.is_some() {
                let did_reset = check_and_reset_period(&mut meta);
                if did_reset {
                    tracing::debug!(
                        key_id = meta.id,
                        period_start = ?meta.period_start,
                        "budget period reset"
                    );
                }
```

And find where `VirtualKeyContext` is inserted (lines ~372-377):
```rust
            request.extensions_mut().insert(VirtualKeyContext {
                key_id: meta.id,
                rate_state: meta.rate_state.clone(),
                allowed_models: meta.allowed_models.clone(),
            });
```

There are two concerns:
1. `did_reset` must be visible outside the `if meta.max_budget_usd.is_some()` block.
2. `period_reset` must carry the new `period_start` string.

Replace the entire budget enforcement + VirtualKeyContext insert block with:

```rust
            // Budget enforcement: lazy period reset then check
            let mut period_reset: Option<String> = None;
            if meta.max_budget_usd.is_some() {
                let did_reset = check_and_reset_period(&mut meta);
                if did_reset {
                    period_reset = meta.period_start.clone();
                    tracing::debug!(
                        key_id = meta.id,
                        period_start = ?meta.period_start,
                        "budget period reset"
                    );
                }
                if let Some(limit) = meta.max_budget_usd {
                    if meta.period_spend_usd >= limit {
                        let reset_at = period_reset_at(&meta);
                        let err_body = serde_json::json!({
                            "error": {
                                "type": "budget_exceeded",
                                "message": format!(
                                    "This API key has exhausted its budget. Current period spend: ${:.2} of ${:.2} limit.",
                                    meta.period_spend_usd, limit
                                ),
                                "budget_limit_usd": limit,
                                "period_spend_usd": meta.period_spend_usd,
                                "budget_duration": meta.budget_duration.as_ref().map(|d| d.as_str()),
                                "period_reset_at": reset_at,
                            }
                        });
                        return Err((StatusCode::TOO_MANY_REQUESTS, Json(err_body)).into_response());
                    }
                }
            }

            // Always insert context for post-response TPM recording and cost tracking.
            request.extensions_mut().insert(VirtualKeyContext {
                key_id: meta.id,
                rate_state: meta.rate_state.clone(),
                allowed_models: meta.allowed_models.clone(),
                period_reset,
            });
```

- [ ] **Step 5: Wire `reset_period_spend` into `record_cost`**

In `crates/proxy/src/cost/mod.rs`, find the `record_cost` function's spawn_blocking closure (lines ~202-220):

```rust
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = db::accumulate_spend(&conn, key_id, cost, input_tokens, output_tokens) {
                tracing::error!(error = %e, key_id, "failed to accumulate spend");
                return;
            }
            // Check spend thresholds after accumulation.
            if let Ok(Some(spend)) = db::get_key_spend(&conn, key_id) {
                if let Some(budget) = spend.max_budget_usd {
                    maybe_fire_spend_alert(
                        key_id,
                        &spend.key_prefix,
                        spend.period_cost_usd,
                        budget,
                        spend.budget_duration.as_deref(),
                    );
                }
            }
        });
```

First, extract `period_reset` from the context. Change the block that extracts `key_id`:

```rust
    if let (Some(shared), Some(ctx)) = (shared, vk_ctx) {
        let db = shared.db.clone();
        let key_id = ctx.key_id;
        let period_reset = ctx.period_reset.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|e| e.into_inner());
            // If the budget period rolled over during auth, reset SQLite first so that
            // accumulate_spend starts from 0 instead of adding to the stale old-period total.
            if let Some(ref new_period_start) = period_reset {
                if let Err(e) = db::reset_period_spend(&conn, key_id, new_period_start) {
                    tracing::error!(error = %e, key_id, "failed to reset period spend");
                }
                crate::cost::reset_alert_level(key_id);
            }
            if let Err(e) = db::accumulate_spend(&conn, key_id, cost, input_tokens, output_tokens) {
                tracing::error!(error = %e, key_id, "failed to accumulate spend");
                return;
            }
            // Check spend thresholds after accumulation.
            if let Ok(Some(spend)) = db::get_key_spend(&conn, key_id) {
                if let Some(budget) = spend.max_budget_usd {
                    maybe_fire_spend_alert(
                        key_id,
                        &spend.key_prefix,
                        spend.period_cost_usd,
                        budget,
                        spend.budget_duration.as_deref(),
                    );
                }
            }
        });
    }
```

- [ ] **Step 6: Run all proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing. `reset_period_spend_zeroes_and_updates_start` must pass. No regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/cost/db.rs crates/proxy/src/cost/mod.rs crates/proxy/src/server/middleware.rs
git commit -m "fix: persist budget period reset to SQLite to prevent desync across restarts"
```

---

### Task 4: Fix Langfuse Start-Time Millisecond Truncation (Low)

**Location:** `crates/proxy/src/integrations/langfuse.rs`

**Root cause:** `iso8601_to_epoch` returns seconds (`u64`). Multiplying by 1000 gives milliseconds, but the sub-second part from the original timestamp is lost, so `end_ms` always ends in `000`. The start time (derived by subtracting `latency_ms`) is off by up to 999ms.

**Fix:** Add a new `iso8601_to_epoch_ms` function that returns milliseconds including the fractional-seconds component (`.SSS` or `.SSSSSS` style). Update `build_generation_payload` to use it.

**Files:**
- Modify: `crates/proxy/src/integrations/langfuse.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block in `langfuse.rs`:

```rust
#[test]
fn iso8601_to_epoch_ms_retains_milliseconds() {
    // Input has 750ms fractional seconds; result must include them.
    let ms = iso8601_to_epoch_ms("2026-03-27T10:15:30.750Z").unwrap();
    // 2026-03-27T10:15:30Z = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000
    let base_ms = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000;
    assert_eq!(ms, base_ms + 750);
}

#[test]
fn iso8601_to_epoch_ms_no_fractional() {
    // No fractional seconds — result equals seconds * 1000.
    let ms = iso8601_to_epoch_ms("2026-03-27T10:15:30Z").unwrap();
    let secs_ms = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000;
    assert_eq!(ms, secs_ms);
}

#[test]
fn build_generation_payload_start_time_precision() {
    use crate::admin::db::RequestLogEntry;
    // endTime = 2026-03-27T10:00:01.500Z (1500ms past second boundary)
    // latency = 400ms -> startTime should be 2026-03-27T10:00:01.100Z
    let entry = RequestLogEntry {
        request_id: "test-precision".to_string(),
        timestamp: "2026-03-27T10:00:01.500Z".to_string(),
        latency_ms: 400,
        backend: "openai".to_string(),
        status_code: 200,
        model_requested: Some("gpt-4o".to_string()),
        model_mapped: None,
        input_tokens: None,
        output_tokens: None,
        cost_usd: None,
        error_message: None,
        key_id: None,
    };
    let payload = build_generation_payload(&entry);
    let start = payload["batch"][0]["body"]["startTime"].as_str().unwrap();
    // Should end in .100Z (1500 - 400 = 1100ms from epoch second)
    assert!(start.ends_with(".100Z"), "startTime was {start}, expected .100Z suffix");
}
```

- [ ] **Step 2: Run tests to confirm `iso8601_to_epoch_ms` fails to compile (function not yet defined)**

```bash
cargo test -p anyllm_proxy integrations::langfuse -- --nocapture 2>&1 | head -30
```

Expected: compile error — `iso8601_to_epoch_ms` not found.

- [ ] **Step 3: Add `iso8601_to_epoch_ms` function**

In `crates/proxy/src/integrations/langfuse.rs`, add immediately after `iso8601_to_epoch`:

```rust
/// Parse ISO 8601 UTC timestamp to Unix epoch milliseconds, retaining sub-second precision.
/// Handles both "2026-03-27T10:15:30Z" (returns seconds * 1000) and
/// "2026-03-27T10:15:30.750Z" / "2026-03-27T10:15:30.750123Z" (retains ms).
pub(crate) fn iso8601_to_epoch_ms(s: &str) -> Option<u64> {
    // Parse whole-second component first.
    let epoch_secs = iso8601_to_epoch(s)?;
    let base_ms = epoch_secs.saturating_mul(1000);

    // Look for fractional seconds: the '.' after position 19 (after "...SSZ" or "...SS.").
    // Timestamp format: "2026-03-27T10:15:30.750Z" → fractional part starts at index 20.
    if s.len() > 20 && s.as_bytes().get(19) == Some(&b'.') {
        let frac_start = 20;
        // Read digits until 'Z' or end
        let frac_end = s[frac_start..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| frac_start + i)
            .unwrap_or(s.len());
        let frac_str = &s[frac_start..frac_end];
        if !frac_str.is_empty() {
            // Normalize to 3 digits (milliseconds): pad with zeros or truncate
            let ms_digits = match frac_str.len() {
                1 => frac_str.parse::<u64>().ok()?.saturating_mul(100),
                2 => frac_str.parse::<u64>().ok()?.saturating_mul(10),
                _ => frac_str[..3].parse::<u64>().ok()?,
            };
            return Some(base_ms + ms_digits);
        }
    }
    Some(base_ms)
}
```

- [ ] **Step 4: Update `build_generation_payload` to use `iso8601_to_epoch_ms`**

In `langfuse.rs`, find `build_generation_payload` (lines ~79-88):

```rust
    let start_time = iso8601_to_epoch(end_time)
        .map(|epoch_secs| {
            // Compute start in milliseconds for sub-second precision.
            let end_ms = epoch_secs.saturating_mul(1000);
            let start_ms = end_ms.saturating_sub(entry.latency_ms);
            crate::admin::db::epoch_to_iso8601_ms(start_ms)
        })
        .unwrap_or_else(|| end_time.clone());
```

Replace with:

```rust
    let start_time = iso8601_to_epoch_ms(end_time)
        .map(|end_ms| {
            let start_ms = end_ms.saturating_sub(entry.latency_ms);
            crate::admin::db::epoch_to_iso8601_ms(start_ms)
        })
        .unwrap_or_else(|| end_time.clone());
```

- [ ] **Step 5: Run all tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing. New precision tests must pass.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/integrations/langfuse.rs
git commit -m "fix: retain millisecond precision in Langfuse start-time calculation"
```

---

## Final Verification

- [ ] **Run full test suite**

```bash
cargo test && cargo clippy -- -D warnings
```

Expected: ~906+ tests passing, 8 ignored, no clippy warnings.

---

## Self-Review Checklist

**Spec coverage:**
- [x] Task 1: XFF spoofing — rightmost IP extraction
- [x] Task 2: IPv6 ULA/link-local SSRF — bitwise checks added
- [x] Task 3: Budget desync — `reset_period_spend` + `VirtualKeyContext.period_reset` + wired in `record_cost`
- [x] Task 4: Langfuse timestamp — `iso8601_to_epoch_ms` + updated call site
- Issue 4 (SQLite mutex / serial writes): intentionally deferred — existing design is an accepted tradeoff for single-binary simplicity; no behavioral bug, only a scalability concern.

**Placeholder scan:** No TBDs, no "add validation later", all code blocks complete.

**Type consistency:**
- `reset_period_spend(conn, key_id, new_period_start)` matches usage in Task 3 Step 5.
- `VirtualKeyContext.period_reset: Option<String>` defined in Task 3 Step 3, used in Steps 4 and 5.
- `iso8601_to_epoch_ms` defined in Task 4 Step 3, used in Step 4 and tested in Step 1.
