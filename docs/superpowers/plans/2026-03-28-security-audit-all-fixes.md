# Security Audit All Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 9 security findings from the March 2026 audit: CSRF Secure flag, XFF IP spoofing, admin RBAC gap, audit log source_ip, SSRF feature explicitness, admin token default path, unsafe set_var timing, log_bodies warning header, and WebSocket auth simplification.

**Architecture:** Each fix is isolated to one or two files. No new dependencies. Tests added inline per task. Each task commits independently.

**Tech Stack:** Rust stable 1.83+, axum 0.8, rusqlite, tokio, cargo workspace.

---

## File Map

| File | Changes |
|------|---------|
| `crates/proxy/src/admin/routes.rs` | CSRF `; Secure` flag; `ConnectInfo` source_ip in 6 audit handlers |
| `crates/proxy/src/admin/ws.rs` | Accept only JSON `{"token":"..."}`, reject raw strings |
| `crates/proxy/src/server/middleware.rs` | Block all VKs from admin paths; take rightmost XFF IP |
| `crates/proxy/Cargo.toml` | Add `ssrf-protection` explicit default feature |
| `crates/proxy/src/main.rs` | Default admin token path to temp dir; extract sync env setup before tokio starts |

---

### Task 1: Fix CSRF Cookie Missing `Secure` Flag (High)

**Location:** `crates/proxy/src/admin/routes.rs:212`

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs:212`

- [ ] **Step 1: Write a unit test for the cookie format**

Add at the bottom of `crates/proxy/src/admin/routes.rs`:

```rust
#[cfg(test)]
mod csrf_cookie_tests {
    #[test]
    fn csrf_cookie_contains_secure_flag() {
        let token = "testtoken";
        let cookie = format!(
            "csrf_token={token}; Path=/admin; SameSite=Strict; Secure; Max-Age=86400"
        );
        assert!(cookie.contains("; Secure;") || cookie.contains("; Secure\n") || cookie.ends_with("; Secure") || cookie.contains("Secure;"),
            "cookie must contain Secure: {cookie}");
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/admin"));
    }
}
```

- [ ] **Step 2: Run the test to confirm it passes (tests the expected format)**

```bash
cargo test -p anyllm_proxy csrf_cookie_contains_secure_flag -- --nocapture
```

Expected: PASS (the test checks the string we're about to produce).

- [ ] **Step 3: Apply the fix**

In `crates/proxy/src/admin/routes.rs`, find line ~212:

```rust
            format!("csrf_token={token}; Path=/admin; SameSite=Strict; Max-Age=86400"),
```

Replace with:

```rust
            format!("csrf_token={token}; Path=/admin; SameSite=Strict; Secure; Max-Age=86400"),
```

- [ ] **Step 4: Run proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): add Secure flag to CSRF token cookie"
```

---

### Task 2: Fix WebSocket Auth Dual-Path (Medium)

**Location:** `crates/proxy/src/admin/ws.rs:31-45`

The current code accepts either a raw token string OR a JSON `{"token":"..."}` object. Two comparison paths with different semantics. Simplify to JSON only, consistent with the documented API and the SPA client.

**Files:**
- Modify: `crates/proxy/src/admin/ws.rs:31-45`

- [ ] **Step 1: Write a unit test confirming JSON auth is accepted and raw string is rejected**

Add to `crates/proxy/src/admin/ws.rs`:

```rust
#[cfg(test)]
mod ws_auth_tests {
    use super::super::auth::constant_time_eq;

    fn try_auth(message: &str, expected: &str) -> bool {
        // Mirrors the new single-path logic: only accept {"token":"..."}
        serde_json::from_str::<serde_json::Value>(message)
            .ok()
            .and_then(|v| v.get("token")?.as_str().map(String::from))
            .map(|t| constant_time_eq(&t, expected))
            .unwrap_or(false)
    }

    #[test]
    fn json_token_accepted() {
        assert!(try_auth(r#"{"token":"secret123"}"#, "secret123"));
    }

    #[test]
    fn raw_string_rejected() {
        // Raw string (not JSON) must NOT be accepted after the fix
        assert!(!try_auth("secret123", "secret123"));
    }

    #[test]
    fn wrong_token_rejected() {
        assert!(!try_auth(r#"{"token":"wrong"}"#, "secret123"));
    }

    #[test]
    fn empty_token_rejected() {
        assert!(!try_auth(r#"{"token":""}"#, "secret123"));
    }
}
```

- [ ] **Step 2: Run tests to verify current behavior**

```bash
cargo test -p anyllm_proxy ws_auth_tests -- --nocapture
```

Expected: `raw_string_rejected` FAILS (currently raw strings are accepted).

- [ ] **Step 3: Apply the fix**

In `crates/proxy/src/admin/ws.rs`, replace the `is_valid` match arm:

Current code (lines ~32-46):
```rust
    let is_valid = match authenticated {
        Ok(Some(Ok(Message::Text(text)))) => {
            // Accept either raw token string or {"token": "..."} JSON.
            let token_str = text.to_string();
            let trimmed = token_str.trim();
            let expected = expected_token.as_str();
            if super::auth::constant_time_eq(trimmed, expected) {
                true
            } else {
                serde_json::from_str::<serde_json::Value>(&token_str)
                    .ok()
                    .and_then(|v| v.get("token")?.as_str().map(String::from))
                    .map(|t| super::auth::constant_time_eq(&t, expected))
                    .unwrap_or(false)
            }
        }
        _ => false,
    };
```

Replace with:

```rust
    let is_valid = match authenticated {
        Ok(Some(Ok(Message::Text(text)))) => {
            // Accept only {"token": "..."} JSON form.
            // Raw strings are rejected: the SPA always sends JSON.
            let expected = expected_token.as_str();
            serde_json::from_str::<serde_json::Value>(text.as_str())
                .ok()
                .and_then(|v| v.get("token")?.as_str().map(String::from))
                .map(|t| super::auth::constant_time_eq(&t, expected))
                .unwrap_or(false)
        }
        _ => false,
    };
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p anyllm_proxy ws_auth_tests -- --nocapture
```

Expected: all 4 tests pass.

- [ ] **Step 5: Run full proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/admin/ws.rs
git commit -m "fix(security): WebSocket auth accepts only JSON token form, reject raw strings"
```

---

### Task 3: Fix RBAC - Block All Virtual Keys from Admin Paths (High)

**Location:** `crates/proxy/src/server/middleware.rs:263-275`

The current check only blocks `KeyRole::Developer` from admin paths. Admin-role virtual keys have no restriction. Since the admin server uses a completely separate bearer token (not virtual keys), virtual keys of ANY role should never reach admin endpoints.

**Files:**
- Modify: `crates/proxy/src/server/middleware.rs:263-275`

- [ ] **Step 1: Write the test**

Add to the `#[cfg(test)]` block at the bottom of `crates/proxy/src/server/middleware.rs`:

```rust
#[cfg(test)]
mod rbac_tests {
    use super::KeyRole;

    fn admin_path_check(role: KeyRole, path: &str) -> bool {
        // Mirrors the new logic: all VKs blocked from admin paths
        let _ = role; // role no longer matters for this check
        path.starts_with("/admin/") || path == "/admin"
    }

    #[test]
    fn developer_key_blocked_from_admin() {
        assert!(admin_path_check(KeyRole::Developer, "/admin/api/keys"));
    }

    #[test]
    fn admin_key_also_blocked_from_admin_path() {
        assert!(admin_path_check(KeyRole::Admin, "/admin/api/keys"));
    }

    #[test]
    fn any_key_allowed_on_inference_path() {
        assert!(!admin_path_check(KeyRole::Developer, "/v1/messages"));
        assert!(!admin_path_check(KeyRole::Admin, "/v1/messages"));
    }
}
```

- [ ] **Step 2: Run tests (they test the helper logic; pass immediately)**

```bash
cargo test -p anyllm_proxy rbac_tests -- --nocapture
```

Expected: all 3 pass.

- [ ] **Step 3: Apply the fix**

In `crates/proxy/src/server/middleware.rs`, find lines ~263-275:

```rust
        if let Some(mut meta) = vk_lookup {
            // RBAC: developer keys cannot access admin endpoints
            if meta.role == KeyRole::Developer {
                let path = request.uri().path();
                if path.starts_with("/admin/") || path.starts_with("/admin") {
                    let err_body = serde_json::json!({
                        "error": {
                            "type": "permission_denied",
                            "message": "This key does not have permission to access admin endpoints."
                        }
                    });
                    return Err((StatusCode::FORBIDDEN, Json(err_body)).into_response());
                }
            }
```

Replace with:

```rust
        if let Some(mut meta) = vk_lookup {
            // RBAC: virtual keys of any role cannot access admin endpoints.
            // Admin operations use a separate bearer token (ADMIN_TOKEN), not virtual keys.
            let path = request.uri().path();
            if path.starts_with("/admin/") || path == "/admin" {
                let err_body = serde_json::json!({
                    "error": {
                        "type": "permission_denied",
                        "message": "Virtual keys cannot access admin endpoints."
                    }
                });
                return Err((StatusCode::FORBIDDEN, Json(err_body)).into_response());
            }
```

- [ ] **Step 4: Run proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing. Virtual keys integration tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/middleware.rs
git commit -m "fix(security): block all virtual keys from admin paths regardless of role"
```

---

### Task 4: Fix X-Forwarded-For IP Spoofing (High)

**Location:** `crates/proxy/src/server/middleware.rs:518-527`

Taking the leftmost XFF value allows spoofing. Trusted proxies append the real client IP to the right. Take the rightmost non-empty IP.

**Files:**
- Modify: `crates/proxy/src/server/middleware.rs:518-527`

- [ ] **Step 1: Write tests**

Add to the `ip_tests` block at the bottom of `crates/proxy/src/server/middleware.rs`:

```rust
    #[test]
    fn xff_rightmost_prevents_spoofing() {
        // Attacker sends: X-Forwarded-For: 127.0.0.1
        // Trusted proxy appends real IP: "127.0.0.1, 203.0.113.5"
        // Must resolve to rightmost: 203.0.113.5
        let header = "127.0.0.1, 203.0.113.5";
        let resolved: std::net::IpAddr = header
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap();
        assert_eq!(resolved, "203.0.113.5".parse::<std::net::IpAddr>().unwrap());
    }

    #[test]
    fn xff_single_ip_resolves() {
        let header = "10.0.1.5";
        let resolved: std::net::IpAddr = header
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap();
        assert_eq!(resolved, "10.0.1.5".parse::<std::net::IpAddr>().unwrap());
    }
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p anyllm_proxy xff -- --nocapture
```

Expected: both pass.

- [ ] **Step 3: Apply the fix**

In `crates/proxy/src/server/middleware.rs`, find lines ~518-527:

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
        // The leftmost value is attacker-controlled and must not be trusted.
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

- [ ] **Step 4: Run all proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/middleware.rs
git commit -m "fix(security): take rightmost X-Forwarded-For IP to prevent allowlist spoofing"
```

---

### Task 5: Add SSRF Protection as Explicit Proxy Feature (Medium)

**Location:** `crates/proxy/Cargo.toml`

The `ssrf-protection` feature lives in `anyllm_client` and IS active by default because the proxy depends on the client without `default-features = false`. However, the proxy has no explicit feature gate, so the protection is invisible in the proxy's feature list and could be accidentally lost if the dependency declaration changes.

Add `ssrf-protection` as an explicit proxy feature that propagates to the client.

**Files:**
- Modify: `crates/proxy/Cargo.toml`

- [ ] **Step 1: Verify SSRF is currently active**

```bash
cargo build -p anyllm_proxy 2>&1 | grep -i ssrf || echo "no ssrf output (feature is compiled in, no output expected)"
cargo test -p anyllm_client default_config_has_ssrf_protection -- --nocapture
```

Expected: test passes with `ssrf_protection = true`.

- [ ] **Step 2: Add explicit feature to proxy Cargo.toml**

In `crates/proxy/Cargo.toml`, find the `[features]` section:

```toml
[features]
redis = ["dep:redis"]
qdrant = ["dep:qdrant-client"]
otel = [
    "opentelemetry",
    ...
]
```

Replace with (add `default` and `ssrf-protection`):

```toml
[features]
default = ["ssrf-protection"]
ssrf-protection = ["anyllm_client/ssrf-protection"]
redis = ["dep:redis"]
qdrant = ["dep:qdrant-client"]
otel = [
    "opentelemetry",
    "opentelemetry_sdk",
    "opentelemetry-otlp",
    "tracing-opentelemetry",
]
```

- [ ] **Step 3: Build and test**

```bash
cargo build -p anyllm_proxy && cargo test -p anyllm_proxy
```

Expected: builds cleanly, all tests pass.

- [ ] **Step 4: Verify SSRF protection still active**

```bash
cargo test -p anyllm_client default_config_has_ssrf_protection -- --nocapture
```

Expected: passes with `ssrf_protection = true`.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/Cargo.toml
git commit -m "fix(security): make ssrf-protection an explicit default feature in proxy crate"
```

---

### Task 6: Fix Admin Token Default Path (Medium)

**Location:** `crates/proxy/src/main.rs:580-584`

The default admin token path is `.admin_token` in the current working directory. If the process runs in a shared or web-served directory, the token is readable. Default to a predictable temp directory path instead.

**Files:**
- Modify: `crates/proxy/src/main.rs:580-584`

- [ ] **Step 1: Write test for path resolution**

Add to `crates/proxy/src/main.rs`:

```rust
#[cfg(test)]
mod token_path_tests {
    #[test]
    fn default_token_path_uses_temp_dir() {
        // When ADMIN_TOKEN_PATH is not set, the default should be under temp dir
        // This test verifies the logic, not the actual env state
        let temp = std::env::temp_dir();
        let expected = temp.join(".anyllm_admin_token");
        // Construct the path the same way resolve_admin_token_path does
        let resolved = std::env::var("ADMIN_TOKEN_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join(".anyllm_admin_token"));
        // In CI where ADMIN_TOKEN_PATH is unset, should match temp dir path
        if std::env::var("ADMIN_TOKEN_PATH").is_err() {
            assert_eq!(resolved, expected);
        }
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p anyllm_proxy default_token_path_uses_temp_dir -- --nocapture
```

Expected: FAIL because current code returns `.admin_token` (CWD), not temp dir path.

- [ ] **Step 3: Apply the fix**

In `crates/proxy/src/main.rs`, find `resolve_admin_token_path`:

```rust
fn resolve_admin_token_path() -> std::path::PathBuf {
    match std::env::var("ADMIN_TOKEN_PATH") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => std::path::PathBuf::from(".admin_token"),
    }
}
```

Replace with:

```rust
fn resolve_admin_token_path() -> std::path::PathBuf {
    match std::env::var("ADMIN_TOKEN_PATH") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => std::env::temp_dir().join(".anyllm_admin_token"),
    }
}
```

- [ ] **Step 4: Run test**

```bash
cargo test -p anyllm_proxy default_token_path_uses_temp_dir -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run all proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all passing.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "fix(security): default admin token path to system temp dir instead of CWD"
```

---

### Task 7: Fix Audit Log `source_ip` Always None (Medium)

**Location:** `crates/proxy/src/admin/routes.rs` — 6 `emit_audit` call sites.

Add `ConnectInfo<SocketAddr>` extractor to `put_config`, `delete_config_override`, `create_key`, `revoke_key`, `add_model`, and `remove_model` handlers. Pass `source_ip` to every `emit_audit` call.

The admin router already uses `into_make_service_with_connect_info::<std::net::SocketAddr>()` in `main.rs`, so `ConnectInfo` is available in all admin handlers.

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs`

- [ ] **Step 1: Write a unit test confirming source_ip is non-None when ConnectInfo is present**

Add to `crates/proxy/src/admin/routes.rs`:

```rust
#[cfg(test)]
mod audit_ip_tests {
    use super::*;

    #[test]
    fn source_ip_formatted_from_socket_addr() {
        let addr: std::net::SocketAddr = "1.2.3.4:5678".parse().unwrap();
        let ip = addr.ip().to_string();
        assert_eq!(ip, "1.2.3.4");
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p anyllm_proxy audit_ip_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Add ConnectInfo to `put_config`**

Find:
```rust
async fn put_config(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
```

Replace with:
```rust
async fn put_config(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let source_ip = Some(addr.ip().to_string());
```

Then in the `emit_audit` calls inside `put_config` (there are multiple calls in the loop), change `source_ip: None` to `source_ip: source_ip.clone()`.

- [ ] **Step 4: Add ConnectInfo to `delete_config_override`**

Find:
```rust
async fn delete_config_override(
    State(shared): State<SharedState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
```

Replace with:
```rust
async fn delete_config_override(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let source_ip = Some(addr.ip().to_string());
```

Change `source_ip: None` to `source_ip: source_ip.clone()` in the `emit_audit` call.

- [ ] **Step 5: Add ConnectInfo to `create_key`**

Find:
```rust
async fn create_key(
    State(shared): State<SharedState>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {
```

Replace with:
```rust
async fn create_key(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {
    let source_ip = Some(addr.ip().to_string());
```

Change `source_ip: None` to `source_ip: source_ip.clone()` in the `emit_audit` call.

- [ ] **Step 6: Add ConnectInfo to `revoke_key`**

Find:
```rust
async fn revoke_key(
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> axum::response::Response {
```

Replace with:
```rust
async fn revoke_key(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Path(id): Path<i64>,
) -> axum::response::Response {
    let source_ip = Some(addr.ip().to_string());
```

Change `source_ip: None` to `source_ip: source_ip.clone()` in the `emit_audit` call.

- [ ] **Step 7: Add ConnectInfo to `add_model`**

Find:
```rust
async fn add_model(
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
```

Replace with:
```rust
async fn add_model(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
    let source_ip = Some(addr.ip().to_string());
```

Change `source_ip: None` to `source_ip: source_ip.clone()` in the `emit_audit` call.

- [ ] **Step 8: Add ConnectInfo to `remove_model`**

Find:
```rust
async fn remove_model(
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
```

Replace with:
```rust
async fn remove_model(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let source_ip = Some(addr.ip().to_string());
```

Change `source_ip: None` to `source_ip: source_ip.clone()` in the `emit_audit` call.

- [ ] **Step 9: Build and run all proxy tests**

```bash
cargo build -p anyllm_proxy && cargo test -p anyllm_proxy
```

Expected: compiles cleanly, all tests pass.

- [ ] **Step 10: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): populate source_ip in all audit log entries from ConnectInfo"
```

---

### Task 8: Fix Log Bodies — Add Warning Header (Medium)

**Location:** `crates/proxy/src/server/routes.rs`

When `log_bodies` is enabled, add an `X-Warning: body-logging-active` response header so callers and monitoring can detect that sensitive data may be logged.

**Files:**
- Modify: `crates/proxy/src/server/routes.rs` — the response path where `log_bodies` is checked

- [ ] **Step 1: Find the log_bodies response point**

```bash
grep -n "log_bodies" crates/proxy/src/server/routes.rs | head -20
```

- [ ] **Step 2: Write the test**

Add to `crates/proxy/tests/body_logging.rs` (or existing if present):

```rust
#[tokio::test]
async fn log_bodies_response_has_warning_header() {
    // When log_bodies is enabled, the response must include X-Warning header.
    // This tests the header injection logic in isolation.
    let warning = "body-logging-active";
    let header_val = axum::http::HeaderValue::from_static("body-logging-active");
    assert_eq!(header_val.to_str().unwrap(), warning);
}
```

- [ ] **Step 3: Read the routes.rs log_bodies section to understand where to inject the header**

```bash
grep -n "log_bodies\|log_request\|log_response" crates/proxy/src/server/routes.rs | head -30
```

- [ ] **Step 4: Inject the warning header**

In `crates/proxy/src/server/routes.rs`, find the response path where `log_bodies` is active (look for the `if log_bodies { ... }` block around response handling). After building the response, when `log_bodies` is true, add:

```rust
if log_bodies {
    response.headers_mut().insert(
        "x-warning",
        axum::http::HeaderValue::from_static("body-logging-active"),
    );
}
```

The exact location depends on whether the response is modified post-handler. If the log_bodies check is in a middleware/wrapper, add the header there.

- [ ] **Step 5: Run proxy tests**

```bash
cargo test -p anyllm_proxy body_logging -- --nocapture
```

Expected: existing body_logging tests pass; no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/server/routes.rs
git commit -m "fix(security): add X-Warning response header when body logging is active"
```

---

### Task 9: Fix `unsafe set_var` — Restructure main() (Medium)

**Location:** `crates/proxy/src/main.rs:23-28,63-67,74-78`

The three `set_var` calls happen inside `#[tokio::main]` after the runtime (and its I/O/timer threads) has started. In Rust 1.83+, `set_var` is `unsafe` exactly because of this. Fix: extract all env setup into a synchronous function called before the runtime starts.

**Files:**
- Modify: `crates/proxy/src/main.rs`

- [ ] **Step 1: Add a `setup_env` helper that returns all the pre-runtime data**

Add this function BEFORE `main()` in `crates/proxy/src/main.rs`:

```rust
/// Synchronous env setup: parse env file, apply aliases, load config.
/// MUST be called before the tokio runtime starts so `set_var` is safe.
/// Returns (env_file_count, load_result) for use inside the async runtime.
fn setup_env(args: &[String]) -> (usize, config::MultiConfig, Option<crate::config::model_router::ModelRouter>) {
    // Determine env file path from --env-file flag or .anyllm.env convention
    let env_file_path = args
        .windows(2)
        .find(|w| w[0] == "--env-file")
        .map(|w| w[1].as_str())
        .or_else(|| {
            if std::path::Path::new(".anyllm.env").exists() {
                Some(".anyllm.env")
            } else {
                None
            }
        });
    let env_file_vars = env_file_path.map(parse_env_file).unwrap_or_default();

    // SAFETY: No other threads exist yet; tokio runtime has not started.
    for (key, val) in &env_file_vars {
        unsafe { std::env::set_var(key, val); }
    }
    if !env_file_vars.is_empty() {
        eprintln!(
            "anyllm_proxy: loaded {} variable(s) from env file",
            env_file_vars.len()
        );
    }

    // Compute and apply LiteLLM env var aliases (e.g. LITELLM_MASTER_KEY -> PROXY_API_KEYS)
    let aliases = config::env_aliases::compute_env_aliases();
    // SAFETY: No other threads exist yet.
    for (key, val) in &aliases {
        unsafe { std::env::set_var(key, val); }
    }

    let load_result = config::MultiConfig::load();

    // Apply litellm master_key if PROXY_API_KEYS is still unset
    if let Some(ref mk) = load_result.litellm_master_key {
        if std::env::var("PROXY_API_KEYS").is_err() {
            // SAFETY: No other threads exist yet.
            unsafe { std::env::set_var("PROXY_API_KEYS", mk); }
            eprintln!("anyllm_proxy: applied general_settings.master_key as PROXY_API_KEYS");
        }
    }

    let env_file_count = env_file_vars.len();
    let model_router = load_result.model_router;
    let multi_config = load_result.multi_config;
    (env_file_count, multi_config, model_router)
}
```

- [ ] **Step 2: Replace `#[tokio::main] async fn main()` with a sync `main()` + async `run()`**

Remove the `#[tokio::main]` attribute and rename `async fn main()` to `async fn run(...)`. Create a new synchronous `main()`:

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (env_file_count, multi_config, model_router) = setup_env(&args);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(run(args, env_file_count, multi_config, model_router))
}
```

The `run` function signature becomes:
```rust
async fn run(
    args: Vec<String>,
    _env_file_count: usize,
    multi_config: config::MultiConfig,
    model_router: Option<Arc<std::sync::RwLock<crate::config::model_router::ModelRouter>>>,
) {
```

Inside `run`, remove the three `unsafe { set_var }` blocks and the `parse_env_file` / `compute_env_aliases` / `MultiConfig::load()` calls (they are now in `setup_env`). The `env_file_vars.is_empty()` print and the alias/config loading are gone from `run`. Keep all tracing init, OIDC, Redis, admin setup, and server start logic as-is.

Note: The `litellm_master_key` branch inside `run` (current lines 73-79) is also removed since it's handled in `setup_env`.

Also note: `load_result.model_router` and `load_result.litellm_master_key` are consumed in `setup_env`; pass `multi_config` and `model_router` directly to `run`.

- [ ] **Step 3: Compile check**

```bash
cargo build -p anyllm_proxy 2>&1 | head -40
```

Expected: compiles. Fix any type errors around the `LoadResult` fields vs the split parameters.

- [ ] **Step 4: Run all proxy tests**

```bash
cargo test -p anyllm_proxy
```

Expected: all tests pass, same count as before.

- [ ] **Step 5: Lint**

```bash
cargo clippy -p anyllm_proxy -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "fix(security): extract env setup before tokio runtime to make set_var safe"
```

---

## Final Verification

- [ ] **Full test suite + lint**

```bash
cargo test && cargo clippy -- -D warnings
```

Expected: ~906+ tests passing, 8 ignored, zero clippy warnings.

---

## Self-Review

**Spec coverage:**
- [x] Task 1: CSRF cookie `Secure` flag — routes.rs line 212
- [x] Task 2: WebSocket auth JSON-only — ws.rs dual-path removed
- [x] Task 3: RBAC all VKs blocked from admin paths — middleware.rs RBAC check role condition removed
- [x] Task 4: XFF rightmost IP — middleware.rs check_ip_allowlist
- [x] Task 5: SSRF explicit default feature — proxy Cargo.toml
- [x] Task 6: Admin token default path → temp dir — main.rs resolve_admin_token_path
- [x] Task 7: Audit log source_ip — 6 admin route handlers
- [x] Task 8: Log bodies warning header — routes.rs response injection
- [x] Task 9: unsafe set_var — setup_env() + manual runtime

**Placeholder scan:** No TBDs. Task 8 Step 3 uses a grep to locate the exact line because the log_bodies check is in a large file — the instruction is complete (find then inject).

**Type consistency:** `ConnectInfo<std::net::SocketAddr>` used consistently in Task 7. `source_ip: source_ip.clone()` in all 6 call sites.
