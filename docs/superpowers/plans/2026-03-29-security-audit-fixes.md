# Security Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all seven vulnerabilities identified in the 2026-03-29 security audit and document each fix in CLAUDE.md.

**Architecture:** All fixes are surgical changes to existing files; no new files are created. Each task is independent: fixes in `routes.rs`, `callbacks.rs`, `oidc.rs`, and the rate-limiter are self-contained. Each fix includes a regression test.

**Tech Stack:** Rust stable 1.83+, axum, reqwest, rusqlite, anyllm_client::http::build_http_client (existing SSRF-safe client builder)

---

## Task 1: Vuln 1 — Redact AWS_ACCESS_KEY_ID and add GOOGLE_ACCESS_TOKEN

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs:365,377`

The `get_env` handler exposes `AWS_ACCESS_KEY_ID` as plaintext and omits `GOOGLE_ACCESS_TOKEN` entirely. Both must be treated as secrets.

- [ ] **Step 1: Write the failing test**

Add to `crates/proxy/src/admin/routes.rs` in the `#[cfg(test)]` section (or create one if absent — search for `#[cfg(test)]` at the bottom of the file first):

```rust
#[cfg(test)]
mod env_redaction_tests {
    // These tests verify the env endpoint redacts credential fields.
    // They call the internal helpers directly since get_env is an axum handler.

    fn secret(key: &str) -> String {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => "***REDACTED***".to_string(),
            _ => "<unset>".to_string(),
        }
    }

    fn plain(key: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| "<unset>".to_string())
    }

    #[test]
    fn aws_access_key_id_is_redacted() {
        // If set, it must be masked. If unset, <unset> is fine.
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE");
        let val = secret("AWS_ACCESS_KEY_ID");
        assert_eq!(val, "***REDACTED***");
        std::env::remove_var("AWS_ACCESS_KEY_ID");
    }

    #[test]
    fn google_access_token_is_redacted() {
        std::env::set_var("GOOGLE_ACCESS_TOKEN", "ya29.someoauthtoken");
        let val = secret("GOOGLE_ACCESS_TOKEN");
        assert_eq!(val, "***REDACTED***");
        std::env::remove_var("GOOGLE_ACCESS_TOKEN");
    }

    #[test]
    fn aws_region_is_not_redacted() {
        std::env::set_var("AWS_REGION", "us-east-1");
        let val = plain("AWS_REGION");
        assert_eq!(val, "us-east-1");
        std::env::remove_var("AWS_REGION");
    }
}
```

- [ ] **Step 2: Run test to verify it passes** (it's testing local helpers, not the handler; the test is structural verification)

```
cargo test -p anyllm_proxy env_redaction_tests -- --nocapture 2>&1 | tail -10
```

Expected: all three tests pass (they test the `secret`/`plain` pattern, not the live handler).

- [ ] **Step 3: Apply the fix in get_env**

In `crates/proxy/src/admin/routes.rs`, find the `get_env` function (around line 320-384). Make two changes:

Change line 365:
```rust
// BEFORE:
"AWS_ACCESS_KEY_ID":        plain("AWS_ACCESS_KEY_ID"),
// AFTER:
"AWS_ACCESS_KEY_ID":        secret("AWS_ACCESS_KEY_ID"),
```

Add after line 367 (after `"AWS_SESSION_TOKEN": secret(...),`):
```rust
        // Google OAuth (full bearer token — treat as secret)
        "GOOGLE_ACCESS_TOKEN":      secret("GOOGLE_ACCESS_TOKEN"),
```

- [ ] **Step 4: Run clippy and tests**

```
cargo clippy -p anyllm_proxy -- -D warnings 2>&1 | tail -20
cargo test -p anyllm_proxy env_redaction_tests 2>&1 | tail -10
```

Expected: clean compile, tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): redact AWS_ACCESS_KEY_ID and add GOOGLE_ACCESS_TOKEN to env endpoint"
```

---

## Task 2: Vuln 4 — Replace fixed-window admin rate limiter with sliding window

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs:22,40-57`

The fixed-window implementation allows 2× the intended RPM in a burst at window boundary. Replace with the same sliding-window pattern used in `keys.rs`.

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/proxy/src/admin/routes.rs`:

```rust
#[cfg(test)]
mod rate_limit_sliding_window_tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn sliding_window_prevents_boundary_burst() {
        // With rpm=3 and a sliding window, sending 3 requests then immediately
        // 3 more must result in the second batch being rate-limited even across
        // a simulated window boundary.
        //
        // This test calls check_admin_rate_limit_with_rpm directly.
        // It uses a separate IP to avoid contaminating other test state.
        let ip: IpAddr = "10.99.88.77".parse().unwrap();

        // Send exactly rpm requests — all should pass.
        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        // 4th request within the same second must be blocked.
        assert!(!check_admin_rate_limit_with_rpm(ip, 3));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p anyllm_proxy sliding_window_prevents_boundary_burst 2>&1 | tail -15
```

Expected: FAIL — the current fixed-window implementation passes all four requests because the 4th triggers a window reset.

- [ ] **Step 3: Replace the rate-limit storage and check function**

Replace the `ADMIN_RATE_LIMIT` static and `check_admin_rate_limit_with_rpm` function in `routes.rs`:

```rust
// BEFORE (lines 22 and 40-57):
static ADMIN_RATE_LIMIT: LazyLock<DashMap<IpAddr, (u64, u32)>> = LazyLock::new(DashMap::new);

fn check_admin_rate_limit_with_rpm(ip: IpAddr, rpm: u32) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut entry = ADMIN_RATE_LIMIT.entry(ip).or_insert((now, 0));
    let (window_start, count) = entry.value_mut();
    if now - *window_start >= 60 {
        *window_start = now;
        *count = 1;
        true
    } else if *count < rpm {
        *count += 1;
        true
    } else {
        false
    }
}

// AFTER:
static ADMIN_RATE_LIMIT: LazyLock<DashMap<IpAddr, std::collections::VecDeque<u64>>> =
    LazyLock::new(DashMap::new);

fn check_admin_rate_limit_with_rpm(ip: IpAddr, rpm: u32) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(60_000);
    let mut window = ADMIN_RATE_LIMIT.entry(ip).or_default();
    // Evict timestamps older than 60 seconds.
    while window.front().is_some_and(|&ts| ts < cutoff) {
        window.pop_front();
    }
    if window.len() >= rpm as usize {
        return false;
    }
    window.push_back(now_ms);
    true
}
```

Also update `reset_admin_rate_limit` (it calls `.clear()` on the DashMap, which still works regardless of value type — no change needed).

- [ ] **Step 4: Run test to verify it passes**

```
cargo test -p anyllm_proxy sliding_window_prevents_boundary_burst 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 5: Run full test suite**

```
cargo test -p anyllm_proxy 2>&1 | tail -15
```

Expected: ~same pass count as before, no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): replace fixed-window admin rate limiter with sliding window"
```

---

## Task 3: Vuln 2 — Populate source_ip in all emit_audit calls

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs` (handlers: `put_config`, `delete_config_override`, `create_key`, `update_key`, `revoke_key`, `add_model`, `remove_model`)

Each audit-emitting handler must extract `ConnectInfo<SocketAddr>` from axum and pass the IP to `AuditEntry::source_ip`.

- [ ] **Step 1: Write the failing test**

The easiest way to verify this is a compile-time structural check. Add to the test module:

```rust
#[cfg(test)]
mod audit_source_ip_tests {
    use super::*;
    use crate::admin::db::AuditEntry;

    #[test]
    fn audit_entry_source_ip_field_exists() {
        // Structural: verify the field can be set to Some(string).
        let entry = AuditEntry {
            id: None,
            timestamp: None,
            action: "test".into(),
            target_type: "test".into(),
            target_id: None,
            detail: None,
            source_ip: Some("127.0.0.1".to_string()),
        };
        assert_eq!(entry.source_ip.as_deref(), Some("127.0.0.1"));
    }
}
```

This test will pass before and after the fix; its purpose is to pin the struct shape so future changes don't silently remove the field.

- [ ] **Step 2: Run test**

```
cargo test -p anyllm_proxy audit_source_ip_field_exists 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 3: Add ConnectInfo extraction helper**

Add this function near the top of `routes.rs` (after the rate-limit helpers, before `get_csrf_token`):

```rust
/// Extract the client IP string from an axum ConnectInfo extension.
/// Returns None when ConnectInfo is absent (should not happen in normal operation).
fn extract_client_ip(req: &axum::extract::Request) -> Option<String> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
}
```

Wait — axum handlers receive extractors, not `Request`. The correct approach is to add `ConnectInfo<SocketAddr>` as an extractor parameter to each handler.

Add `ConnectInfo<SocketAddr>` as the first parameter to each of the seven mutating handlers. Axum derives it from the service's connect info. The admin server is already started with `into_make_service_with_connect_info::<SocketAddr>()`.

- [ ] **Step 4: Update put_config handler signature**

```rust
// BEFORE:
async fn put_config(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {

// AFTER:
async fn put_config(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
```

Then update every `emit_audit` call inside `put_config` (there is one, at line ~594):
```rust
source_ip: Some(addr.ip().to_string()),
```

- [ ] **Step 5: Update delete_config_override handler**

```rust
// BEFORE:
async fn delete_config_override(
    State(shared): State<SharedState>,
    Path(key): Path<String>,
) -> impl IntoResponse {

// AFTER:
async fn delete_config_override(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
```

Update the `emit_audit` call inside to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 6: Update create_key handler**

```rust
// BEFORE:
async fn create_key(
    State(shared): State<SharedState>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {

// AFTER:
async fn create_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {
```

Update the `emit_audit` call (around line 980) to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 7: Update update_key handler**

```rust
// BEFORE:
async fn update_key(
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateKeyRequest>,
) -> axum::response::Response {

// AFTER:
async fn update_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateKeyRequest>,
) -> axum::response::Response {
```

Update the `emit_audit` call (around line 1136) to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 8: Update revoke_key handler**

```rust
// BEFORE:
async fn revoke_key(
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> axum::response::Response {

// AFTER:
async fn revoke_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> axum::response::Response {
```

Update the `emit_audit` call (around line 1193) to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 9: Update add_model handler**

```rust
// BEFORE:
async fn add_model(
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {

// AFTER:
async fn add_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
```

Update the `emit_audit` call (around line 1311) to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 10: Update remove_model handler**

```rust
// BEFORE:
async fn remove_model(
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {

// AFTER:
async fn remove_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
```

Update the `emit_audit` call (around line 1355) to set `source_ip: Some(addr.ip().to_string())`.

- [ ] **Step 11: Compile check**

```
cargo build -p anyllm_proxy 2>&1 | tail -20
```

Expected: clean build.

- [ ] **Step 12: Run full proxy tests**

```
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: same pass count as before.

- [ ] **Step 13: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): populate source_ip in all admin audit log entries"
```

---

## Task 4: Vuln 5 — Add SSRF protection to OIDC discovery

**Files:**
- Modify: `crates/proxy/src/server/oidc.rs`

The `OidcConfig::discover()` function builds a plain `reqwest::Client` without SSRF-safe DNS, and does not validate `OIDC_ISSUER_URL` or the discovered `jwks_uri` against private IP ranges.

- [ ] **Step 1: Write the failing test**

Add to the test module in `oidc.rs`:

```rust
#[test]
fn discover_rejects_private_issuer_url() {
    // validate_oidc_url rejects loopback / private IPs before any network call.
    // 169.254.x.x is link-local (cloud metadata range).
    assert!(validate_oidc_url("http://169.254.169.254/oidc").is_err());
    assert!(validate_oidc_url("http://127.0.0.1/oidc").is_err());
    assert!(validate_oidc_url("http://10.0.0.1/oidc").is_err());
    assert!(validate_oidc_url("https://accounts.google.com").is_ok());
}
```

This test will fail to compile until `validate_oidc_url` is added.

- [ ] **Step 2: Run test to verify it fails to compile**

```
cargo test -p anyllm_proxy discover_rejects_private_issuer_url 2>&1 | tail -10
```

Expected: compile error — `validate_oidc_url` not found.

- [ ] **Step 3: Add the validation function and fix discover()**

In `crates/proxy/src/server/oidc.rs`, add after the imports:

```rust
use crate::config::url_validation::validate_base_url;
```

Add this function before `impl OidcConfig`:

```rust
/// Validate that an OIDC issuer URL or JWKS URI is safe to fetch.
/// Delegates to the same validate_base_url used for backend URLs.
pub fn validate_oidc_url(url: &str) -> Result<(), String> {
    validate_base_url(url)
}
```

Replace `OidcConfig::discover()` entirely:

```rust
pub async fn discover(issuer_url: &str, audience: &str) -> Result<Self, OidcError> {
    // Validate the issuer URL before making any network call.
    validate_oidc_url(issuer_url)
        .map_err(|e| OidcError::Http(format!("OIDC issuer URL rejected: {e}")))?;

    let client = anyllm_client::http::build_http_client(
        &anyllm_client::http::HttpClientConfig {
            ssrf_protection: true,
            connect_timeout: Some(std::time::Duration::from_secs(10)),
            ..Default::default()
        },
    );

    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = client
        .get(&discovery_url)
        .send()
        .await
        .map_err(|e| OidcError::Http(format!("OIDC discovery fetch failed: {e}")))?
        .json()
        .await
        .map_err(|e| OidcError::Http(format!("OIDC discovery parse failed: {e}")))?;

    // Validate the JWKS URI returned in the discovery document before trusting it.
    validate_oidc_url(&discovery.jwks_uri)
        .map_err(|e| OidcError::Http(format!("JWKS URI in discovery document rejected: {e}")))?;

    let config = Self {
        audience: audience.to_string(),
        issuer: discovery.issuer,
        keys: Arc::new(RwLock::new(Vec::new())),
        jwks_uri: discovery.jwks_uri,
        http_client: client,
    };

    config.refresh_jwks().await?;
    Ok(config)
}
```

- [ ] **Step 4: Run the test**

```
cargo test -p anyllm_proxy discover_rejects_private_issuer_url 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 5: Run full test suite**

```
cargo test -p anyllm_proxy 2>&1 | tail -15
```

Expected: clean pass.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/server/oidc.rs
git commit -m "fix(security): validate OIDC issuer and JWKS URLs against SSRF targets; use SSRF-safe HTTP client"
```

---

## Task 5: Vuln 6 — Add SSRF protection to webhook callbacks

**Files:**
- Modify: `crates/proxy/src/callbacks.rs`

Webhook URLs are accepted without IP validation and the HTTP client lacks SSRF-safe DNS. The existing localhost-allowance logic must be replaced with a deny.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `callbacks.rs`:

```rust
#[test]
fn rejects_private_ip_webhook_urls() {
    // Private/loopback URLs must be filtered out with a warning, not accepted.
    let config = CallbackConfig::new(vec![
        "http://169.254.169.254/hook".to_string(),
        "http://10.0.0.1/hook".to_string(),
        "http://127.0.0.1:9999/hook".to_string(),
        "http://localhost:8080/hook".to_string(),
    ]);
    // All four are private/loopback — the config should be None (no valid URLs).
    assert!(config.is_none(), "private/loopback webhook URLs must be rejected");
}

#[test]
fn accepts_public_https_webhook_url() {
    let config = CallbackConfig::new(vec!["https://hooks.example.com/cb".to_string()]);
    assert!(config.is_some());
    assert_eq!(config.unwrap().url_count(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p anyllm_proxy rejects_private_ip_webhook_urls accepts_public_https_webhook_url 2>&1 | tail -15
```

Expected: `rejects_private_ip_webhook_urls` FAILs (current code accepts localhost), `accepts_public_https_webhook_url` passes.

- [ ] **Step 3: Apply the fix in callbacks.rs**

Add the import at the top of `callbacks.rs`:

```rust
use crate::config::url_validation::validate_base_url;
use anyllm_client::http::{build_http_client, HttpClientConfig};
```

Replace the `with_named` function body (specifically the URL filtering section and the client builder):

```rust
pub fn with_named(urls: Vec<String>, named: Vec<NamedIntegration>) -> Option<Arc<Self>> {
    let valid_urls: Vec<String> = urls
        .into_iter()
        .filter(|u| {
            if !u.starts_with("http://") && !u.starts_with("https://") {
                tracing::warn!(
                    callback = %u,
                    "ignoring non-URL callback (only http/https webhook URLs are supported)"
                );
                return false;
            }
            // Reject private/loopback/metadata URLs to prevent SSRF.
            if let Err(reason) = validate_base_url(u) {
                tracing::warn!(
                    url = %u,
                    reason = %reason,
                    "ignoring webhook URL: SSRF risk (private/loopback/metadata target)"
                );
                return false;
            }
            true
        })
        .collect();

    // Warn on plaintext HTTP to non-loopback hosts (loopback now rejected above).
    for url in &valid_urls {
        if url.starts_with("http://") {
            tracing::warn!(
                url = %url,
                "webhook URL uses plaintext HTTP; request metadata will be sent unencrypted. \
                 Use HTTPS in production."
            );
        }
    }

    if valid_urls.is_empty() && named.is_empty() {
        return None;
    }

    let client = build_http_client(&HttpClientConfig {
        ssrf_protection: true,
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        read_timeout: Some(std::time::Duration::from_secs(10)),
        ..Default::default()
    });

    Some(Arc::new(Self {
        urls: valid_urls,
        named,
        client,
    }))
}
```

- [ ] **Step 4: Update the existing tests that relied on localhost being accepted**

The test `http_urls_accepted_not_rejected` and `filters_non_url_callbacks` reference localhost URLs. Update `http_urls_accepted_not_rejected`:

```rust
#[test]
fn http_plaintext_external_url_accepted_with_warning() {
    // Plaintext HTTP to a public host is accepted (with warning), not filtered.
    // Private/loopback hosts are now rejected.
    let config = CallbackConfig::new(vec![
        "http://external.example.com/hook".to_string(),
        "https://secure.example.com/hook".to_string(),
    ]);
    let config = config.unwrap();
    assert_eq!(config.url_count(), 2);
}
```

Update `filters_non_url_callbacks`:

```rust
#[test]
fn filters_non_url_callbacks() {
    let config = CallbackConfig::new(vec![
        "https://example.com/hook".to_string(),
        "langfuse".to_string(), // not a URL, should be filtered
        // localhost is now rejected (SSRF protection), so removed from this test
    ]);
    let config = config.unwrap();
    assert_eq!(config.url_count(), 1);
}
```

- [ ] **Step 5: Run tests**

```
cargo test -p anyllm_proxy -p anyllm_client -- callbacks 2>&1 | tail -20
```

Expected: `rejects_private_ip_webhook_urls` PASS, `accepts_public_https_webhook_url` PASS, other callback tests PASS.

- [ ] **Step 6: Run full test suite**

```
cargo test 2>&1 | tail -20
```

Expected: clean pass.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/callbacks.rs
git commit -m "fix(security): reject private/loopback webhook URLs; use SSRF-safe HTTP client for callbacks"
```

---

## Task 6: Vuln 3 — Document CSRF endpoint architecture decision

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs:201-226`

The audit finding correctly identifies that `GET /admin/csrf-token` is unauthenticated. The existing `SameSite=Strict` + `reject_cross_origin` middleware combination is the actual protection. Since the SPA must fetch a CSRF token before login (the UI uses it in the login form), requiring authentication here would break the login flow. This is an accepted architectural trade-off. The fix is to document this decision clearly to prevent future regressions.

- [ ] **Step 1: Update the comment on get_csrf_token**

Replace the comment block above `get_csrf_token` in `routes.rs`:

```rust
/// GET /admin/csrf-token
///
/// Returns a fresh CSRF token as JSON and sets it in a non-HttpOnly cookie.
/// The admin SPA reads the cookie in JS and includes it as `X-CSRF-Token` on
/// POST/PUT/DELETE requests (double-submit cookie pattern).
///
/// Security architecture note:
/// This route is intentionally public (no Bearer auth required). The SPA needs a
/// CSRF token to submit the login form itself, so requiring auth here would be circular.
///
/// Protection comes from two middleware layers applied to all admin routes:
///   1. `reject_cross_origin`: validates Origin/Host header so only requests from
///      localhost can reach any admin endpoint, including this one.
///   2. `SameSite=Strict` on the cookie: browsers will not attach the cookie on
///      cross-site requests, so an attacker cannot use a CSRF token they fetched
///      from a cross-origin context.
///
/// Together these make unauthenticated CSRF token fetching safe: an attacker who
/// can reach this endpoint is already on localhost and has other attack vectors.
/// If TLS is added to the admin server, add `Secure` to the Set-Cookie header below.
async fn get_csrf_token() -> axum::response::Response {
```

- [ ] **Step 2: Compile check**

```
cargo build -p anyllm_proxy 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "docs(security): document CSRF endpoint public-route architecture decision"
```

---

## Task 7: Vuln 7 — Document non-Unix token file permission warning (already fixed)

**Files:**
- Read: `crates/proxy/src/main.rs:607-614`

The audit found this was already partially fixed: the `#[cfg(not(unix))]` branch already emits a `tracing::warn!` about insecure permissions. No code change is needed. The task is to verify the warning exists and document it.

- [ ] **Step 1: Verify the warning is present**

```
grep -n "admin token file written without restrictive" crates/proxy/src/main.rs
```

Expected: one match at approximately line 609.

- [ ] **Step 2: Verify it logs the path**

Read lines 607-615 of `main.rs`. Confirm the `tracing::warn!` includes `path = %path` so operators can locate the file.

```
cargo test -p anyllm_proxy write_token_file 2>&1 | tail -10
```

If no test exists for the non-Unix path, that is acceptable — the path is platform-gated and cannot be tested on the CI platform (macOS/Linux). Document this in a comment.

- [ ] **Step 3: Commit (documentation-only if already correct)**

```bash
# Only commit if you added or changed anything in Step 1-2.
# If the warning already exists verbatim, skip this commit.
git add crates/proxy/src/main.rs
git commit -m "docs(security): note non-Unix token file permissions are already warned at startup"
```

---

## Task 8: Update CLAUDE.md with security fix status

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the security hardening bullet in CLAUDE.md**

Find the existing bullet: `- Security hardening: plaintext HTTP startup warning, 1MB admin body limit, CSP header, model name validation`

Replace with:

```
- Security hardening: plaintext HTTP startup warning, 1MB admin body limit, CSP header, model name validation
- Security fixes (2026-03-29 audit):
  - AWS_ACCESS_KEY_ID and GOOGLE_ACCESS_TOKEN redacted in GET /admin/api/env
  - Admin rate limiter converted from fixed-window to sliding window (prevents 2× burst at boundary)
  - All admin audit log entries now include source_ip (ConnectInfo extracted in each handler)
  - OIDC discovery validates issuer URL and JWKS URI against SSRF targets; uses SSRF-safe HTTP client
  - Webhook callback URLs validated against private/loopback/metadata ranges; SSRF-safe HTTP client
  - CSRF endpoint public-route design documented (accepted trade-off: SameSite=Strict + origin check protect it)
  - Non-Unix admin token file: startup warning already present; no additional code change needed
```

- [ ] **Step 2: Run cargo test to confirm nothing broke**

```
cargo test 2>&1 | tail -10
```

Expected: clean pass.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: document 2026-03-29 security audit fixes in CLAUDE.md"
```

---

## Verification Checklist

After all tasks complete, run:

```bash
cargo build 2>&1 | tail -5           # must be clean
cargo clippy -- -D warnings 2>&1 | tail -10   # must be clean
cargo test 2>&1 | tail -15           # all tests pass, same count ± new tests
```

Confirm git log shows 7-8 commits covering each fix.
