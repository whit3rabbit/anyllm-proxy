# Security Audit 7-Finding Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 7 security findings from the 2026-03-31 audit: model allowlist bypass on passthrough/Gemini paths, batch webhook SSRF, unbounded CSRF token store, world-readable admin token on non-Unix, unvalidated timestamp query params, overly broad JWT heuristic, and fragile unsafe set_var.

**Architecture:** All fixes are targeted single- or two-file changes. No new crate dependencies. The most impactful changes are the model allowlist addition (2 files) and the CSRF store swap from unbounded DashMap to a bounded moka Cache (4 files). Everything else is a guard or tightened condition.

**Tech Stack:** Rust stable 1.83+, axum, moka 0.12 (already a dependency), reqwest, DashMap

---

## Task 1: Enforce model allowlist in Anthropic passthrough handler (Finding 2, HIGH)

**Files:**
- Modify: `crates/proxy/src/server/passthrough.rs`

The passthrough handler currently ignores the `VirtualKeyContext` from request extensions entirely.
It peeks at the `stream` field; extend that peek to also capture `model` so the allowlist can be checked.

- [ ] **Step 1: Add vk_ctx parameter and model peek to `anthropic_passthrough`**

In `crates/proxy/src/server/passthrough.rs`, replace the current handler signature and `StreamPeek` block:

```rust
pub(crate) async fn anthropic_passthrough(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Peek at `stream` and `model` before forwarding.
    // Full deserialization would be wasteful for image-heavy requests.
    #[derive(serde::Deserialize)]
    struct BodyPeek {
        #[serde(default)]
        stream: bool,
        model: Option<String>,
    }
    let peek = serde_json::from_slice::<BodyPeek>(&body)
        .unwrap_or(BodyPeek { stream: false, model: None });
    let is_stream = peek.stream;

    // Enforce model allowlist for virtual keys.
    if let Some(axum::Extension(ref ctx)) = vk_ctx {
        if let Some(ref m) = peek.model {
            if !super::policy::is_model_allowed(m, &ctx.allowed_models) {
                let err = mapping::errors_map::create_anthropic_error(
                    anthropic::ErrorType::PermissionError,
                    format!("Model '{}' is not allowed for this API key.", m),
                    None,
                );
                return (StatusCode::FORBIDDEN, Json(err)).into_response();
            }
        }
    }
```

The rest of the function body (the `if is_stream { ... } else { ... }` block and `passthrough_error_to_response`) stays unchanged.

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p anyllm_proxy 2>&1 | tail -20
```

Expected: no errors, possibly 0 warnings.

- [ ] **Step 3: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -30
```

Expected: same pass count as before (no regressions).

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/server/passthrough.rs
git commit -m "fix(security): enforce model allowlist on Anthropic passthrough path"
```

---

## Task 2: Enforce model allowlist in Gemini native handler (Finding 2, HIGH)

**Files:**
- Modify: `crates/proxy/src/server/gemini_native.rs`

`gemini_native_handler` already parses the full body. It only needs the `vk_ctx` extension injected and one guard block.

- [ ] **Step 1: Add vk_ctx parameter and allowlist check**

In `crates/proxy/src/server/gemini_native.rs`, change the handler signature and add the check right after `state.metrics.record_request()`:

Old signature (line 25-28):
```rust
pub(crate) async fn gemini_native_handler(
    State(state): State<AppState>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
```

New signature:
```rust
pub(crate) async fn gemini_native_handler(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
```

After `state.metrics.record_request();` (currently line 46), insert:
```rust
    // Enforce model allowlist for virtual keys.
    if let Some(axum::Extension(ref ctx)) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", body.model),
                None,
            );
            return (
                axum::http::StatusCode::FORBIDDEN,
                axum::response::Json(err),
            )
                .into_response();
        }
    }
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p anyllm_proxy 2>&1 | tail -20
```

Expected: clean build.

- [ ] **Step 3: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -30
```

Expected: no regressions.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/server/gemini_native.rs
git commit -m "fix(security): enforce model allowlist on Gemini native handler path"
```

---

## Task 3: Validate webhook_url in create_batch to block SSRF (Finding 1, HIGH)

**Files:**
- Modify: `crates/proxy/src/batch/routes.rs`

Currently `BatchSubmission.webhook_url` is hardcoded `None`. This task adds the optional field to `CreateBatchRequest` with SSRF validation, closing the gate before the field is ever exposed to clients.

- [ ] **Step 1: Add webhook_url to CreateBatchRequest and validate it**

In `crates/proxy/src/batch/routes.rs`:

1. Add `webhook_url: Option<String>` to `CreateBatchRequest` (after `metadata`):

```rust
#[derive(Deserialize)]
pub struct CreateBatchRequest {
    pub input_file_id: String,
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_completion_window")]
    pub completion_window: String,
    pub metadata: Option<serde_json::Value>,
    /// Optional per-batch webhook URL. Must be a public HTTPS/HTTP URL.
    /// Validated against SSRF rules before use.
    pub webhook_url: Option<String>,
}
```

2. In `create_batch`, before constructing `BatchSubmission`, add validation (insert after `let items: Vec<SubmissionItem> = ...` block, before `let execution_mode = ...`):

```rust
    // Validate webhook_url to prevent SSRF: reject private/loopback/metadata targets.
    if let Some(ref url) = req.webhook_url {
        if let Err(e) = crate::config::url_validation::validate_base_url(url) {
            return bad_request(&format!("Invalid webhook_url: {e}"));
        }
    }
```

3. Change `webhook_url: None` in `BatchSubmission` to `webhook_url: req.webhook_url.clone()`:

```rust
    let submission = BatchSubmission {
        items,
        execution_mode,
        input_file_id: req.input_file_id.clone(),
        key_id: None,
        webhook_url: req.webhook_url.clone(),
        metadata: req.metadata.clone(),
        priority: 0,
    };
```

- [ ] **Step 2: Write a test for the validation**

Add to the test module in `crates/proxy/src/batch/routes.rs` (or create one if absent):

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn validate_webhook_url_rejects_private_ip() {
        let result = crate::config::url_validation::validate_base_url("http://169.254.169.254/metadata");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private/loopback"));
    }

    #[test]
    fn validate_webhook_url_accepts_public_https() {
        let result = crate::config::url_validation::validate_base_url("https://hooks.example.com/notify");
        assert!(result.is_ok());
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p anyllm_proxy batch::routes 2>&1
```

Expected: both new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/batch/routes.rs
git commit -m "fix(security): validate webhook_url against SSRF rules in create_batch"
```

---

## Task 4: Replace unbounded CSRF token DashMap with bounded moka cache (Finding 3, MEDIUM)

**Files:**
- Modify: `crates/proxy/src/admin/state.rs`
- Modify: `crates/proxy/src/admin/routes.rs`
- Modify: `crates/proxy/src/main.rs`
- Modify: `crates/proxy/src/cost/mod.rs`

`moka` is already in `Cargo.toml` with the `future` feature. `moka::sync::Cache` is available with no additional dependencies.

- [ ] **Step 1: Change the field type in SharedState**

In `crates/proxy/src/admin/state.rs`:

1. Add the import at the top (after existing imports):

```rust
use std::time::Duration;
```

2. Change the `issued_csrf_tokens` field type from `Arc<DashMap<String, ()>>` to `moka::sync::Cache<String, ()>`:

```rust
    /// Set of CSRF tokens issued by GET /admin/csrf-token that have not yet
    /// been consumed. Tokens are removed on first successful CSRF validation
    /// (one-time use), preventing replay across multiple mutating requests.
    /// Bounded to 1,000 entries with 24-hour TTL to prevent memory exhaustion.
    pub issued_csrf_tokens: moka::sync::Cache<String, ()>,
```

3. Update `new_for_test()` to construct a moka cache instead of DashMap:

```rust
            issued_csrf_tokens: moka::sync::Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(86_400))
                .build(),
```

- [ ] **Step 2: Update routes.rs to use get + invalidate instead of remove**

In `crates/proxy/src/admin/routes.rs`, find the CSRF validation block (around line 225-232) that does:

```rust
if shared.issued_csrf_tokens.remove(header_token).is_none() {
```

Replace with:

```rust
if shared.issued_csrf_tokens.get(header_token).is_none() {
```

and add `shared.issued_csrf_tokens.invalidate(header_token);` immediately after the closing brace of the `if` block (i.e., after the error-return block, to consume the token on success):

The pattern should become:
```rust
// Verify the token was issued by this server (not replayed from a stolen cookie).
if shared.issued_csrf_tokens.get(header_token).is_none() {
    return (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": {
                "type": "forbidden",
                "message": "CSRF token was not issued by this server or has already been consumed"
            }
        })),
    )
        .into_response();
}
// Consume the token (one-time use).
shared.issued_csrf_tokens.invalidate(header_token);
```

Remove the old code that called `.remove(...)` and the associated guard block. Check around lines 225-240 for the original pattern to ensure you replace the right block.

- [ ] **Step 3: Update main.rs construction**

In `crates/proxy/src/main.rs`, find (around line 435):

```rust
issued_csrf_tokens: Arc::new(dashmap::DashMap::new()),
```

Replace with:

```rust
issued_csrf_tokens: moka::sync::Cache::builder()
    .max_capacity(1_000)
    .time_to_live(std::time::Duration::from_secs(86_400))
    .build(),
```

- [ ] **Step 4: Update cost/mod.rs test construction**

In `crates/proxy/src/cost/mod.rs`, find (around line 438):

```rust
issued_csrf_tokens: Arc::new(dashmap::DashMap::new()),
```

Replace with:

```rust
issued_csrf_tokens: moka::sync::Cache::builder()
    .max_capacity(1_000)
    .time_to_live(std::time::Duration::from_secs(86_400))
    .build(),
```

- [ ] **Step 5: Also update the inline test in routes.rs**

In `crates/proxy/src/admin/routes.rs`, search for the test that does `issued_csrf_tokens.insert(...)` (around line 1895). Update it to use the moka builder. Look for the test that creates a `SharedState` inline and constructs `issued_csrf_tokens: Arc::new(...)`:

```rust
issued_csrf_tokens: moka::sync::Cache::builder()
    .max_capacity(1_000)
    .time_to_live(std::time::Duration::from_secs(86_400))
    .build(),
```

- [ ] **Step 6: Build and test**

```bash
cargo build -p anyllm_proxy 2>&1 | tail -30
cargo test -p anyllm_proxy 2>&1 | tail -30
```

Expected: clean build, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/admin/state.rs crates/proxy/src/admin/routes.rs \
        crates/proxy/src/main.rs crates/proxy/src/cost/mod.rs
git commit -m "fix(security): replace unbounded CSRF DashMap with moka cache (1000 entries, 24h TTL)"
```

---

## Task 5: Require explicit ADMIN_TOKEN on non-Unix platforms (Finding 4, MEDIUM)

**Files:**
- Modify: `crates/proxy/src/main.rs`

The current non-Unix branch in `write_token_file` warns and creates a world-readable file. Change it to return an `Err` so the caller panics and the operator is forced to set `ADMIN_TOKEN` explicitly.

- [ ] **Step 1: Replace the non-Unix branch in write_token_file**

In `crates/proxy/src/main.rs`, find the `#[cfg(not(unix))]` branch inside `write_token_file` (around lines 793-801):

Old:
```rust
    #[cfg(not(unix))]
    let mut file = {
        tracing::warn!(
            path = %path,
            "admin token file written without restrictive permissions (non-Unix platform); \
             secure this file manually or set ADMIN_TOKEN_PATH to a protected location"
        );
        std::fs::File::create(path)?
    };
```

New:
```rust
    #[cfg(not(unix))]
    let mut file: std::fs::File = {
        // On non-Unix platforms, file permissions cannot be set to owner-only
        // at creation time. Returning an error forces the caller to panic,
        // requiring the operator to set ADMIN_TOKEN explicitly.
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "auto-generating the admin token file is not supported on non-Unix platforms; \
             set the ADMIN_TOKEN environment variable explicitly",
        ));
    };
```

- [ ] **Step 2: Build (Unix and cross-compile check)**

```bash
cargo build -p anyllm_proxy 2>&1 | tail -20
```

Expected: clean build. The `#[cfg(not(unix))]` block is dead code on macOS/Linux but compiles correctly.

- [ ] **Step 3: Test that the panic message is clear**

The existing caller already panics when `write_token_file` fails:

```rust
if let Err(e) = write_token_file(&token_path, &token) {
    panic!(
        "Cannot write admin token to {token_path}: {e}. \
         Set ADMIN_TOKEN env var explicitly or ensure the path is writable."
    );
```

The `{e}` will now say "auto-generating the admin token file is not supported on non-Unix platforms; set the ADMIN_TOKEN environment variable explicitly", which is clear.

Run:
```bash
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: no regressions.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "fix(security): require explicit ADMIN_TOKEN on non-Unix (token file unsupported)"
```

---

## Task 6: Validate since/until timestamps in admin log query handlers (Finding 5, MEDIUM)

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs`

Two handlers accept user-supplied `since`/`until` query strings and pass them directly to SQLite comparisons. Add a format check that prevents arbitrary strings (no injection risk, but prevents full-table scan DoS by ensuring the strings parse as valid ISO 8601 dates).

- [ ] **Step 1: Add the validator helper function**

In `crates/proxy/src/admin/routes.rs`, add this helper near the top of the file (after the imports, before the first `async fn`):

```rust
/// Validate that a string looks like an ISO 8601 / RFC 3339 timestamp.
/// Accepts YYYY-MM-DD (date only) or YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM].
/// Does not check calendar validity (e.g., Feb 30) — the goal is to
/// reject obviously malformed strings that bypass the timestamp index.
fn is_valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 {
        return false;
    }
    // YYYY-MM-DD
    b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && (b.len() == 10
            || (b.len() >= 19
                && (b[10] == b'T' || b[10] == b' ')
                && b[11..13].iter().all(|c| c.is_ascii_digit())
                && b[13] == b':'
                && b[14..16].iter().all(|c| c.is_ascii_digit())
                && b[16] == b':'
                && b[17..19].iter().all(|c| c.is_ascii_digit())))
}
```

- [ ] **Step 2: Add validation in get_requests**

In `get_requests` (around line 967), after `let since = params.since;` and `let until = params.until;`, add:

```rust
    if let Some(ref s) = since {
        if !is_valid_timestamp(s) {
            return Json(serde_json::json!({
                "error": "invalid 'since' value; expected ISO 8601 date or datetime",
                "requests": [],
            }));
        }
    }
    if let Some(ref u) = until {
        if !is_valid_timestamp(u) {
            return Json(serde_json::json!({
                "error": "invalid 'until' value; expected ISO 8601 date or datetime",
                "requests": [],
            }));
        }
    }
```

- [ ] **Step 3: Add validation in get_audit_log**

In `get_audit_log` (around line 1605), after extracting `since` and `until`, add the same guard:

```rust
    if let Some(ref s) = since {
        if !is_valid_timestamp(s) {
            return Json(serde_json::json!({
                "error": "invalid 'since' value; expected ISO 8601 date or datetime",
                "entries": [],
            }));
        }
    }
    if let Some(ref u) = until {
        if !is_valid_timestamp(u) {
            return Json(serde_json::json!({
                "error": "invalid 'until' value; expected ISO 8601 date or datetime",
                "entries": [],
            }));
        }
    }
```

- [ ] **Step 4: Write unit tests for the validator**

Add at the bottom of `routes.rs` in the existing test module (or create a new inline test module):

```rust
#[cfg(test)]
mod timestamp_tests {
    use super::is_valid_timestamp;

    #[test]
    fn accepts_date_only() {
        assert!(is_valid_timestamp("2026-03-31"));
    }

    #[test]
    fn accepts_datetime_utc() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00Z"));
    }

    #[test]
    fn accepts_datetime_no_tz() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_timestamp(""));
    }

    #[test]
    fn rejects_arbitrary_string() {
        assert!(!is_valid_timestamp("not-a-date"));
    }

    #[test]
    fn rejects_sql_injection_attempt() {
        assert!(!is_valid_timestamp("'; DROP TABLE request_log; --"));
    }

    #[test]
    fn rejects_too_short() {
        assert!(!is_valid_timestamp("2026-03"));
    }
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p anyllm_proxy timestamp_tests 2>&1
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all timestamp_tests pass, no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(security): validate since/until timestamps in admin log query handlers"
```

---

## Task 7: Tighten looks_like_jwt to require Base64url segments (Finding 6, LOW)

**Files:**
- Modify: `crates/proxy/src/server/oidc.rs`

The current check (`credential.matches('.').count() == 2 && credential.len() > 32`) would match any string with two dots, including API keys that contain dots. A real JWT has three Base64url segments.

- [ ] **Step 1: Replace looks_like_jwt**

In `crates/proxy/src/server/oidc.rs`, replace the current `looks_like_jwt` function (around line 240-243):

Old:
```rust
/// Check if a credential looks like a JWT (three base64url segments separated by dots).
pub fn looks_like_jwt(credential: &str) -> bool {
    credential.matches('.').count() == 2 && credential.len() > 32
}
```

New:
```rust
/// Check if a credential looks like a JWT (three Base64url segments separated by dots).
/// All characters in each segment must be `[A-Za-z0-9_-]` — the Base64url alphabet.
/// This prevents API keys that happen to contain two dots from being mistakenly
/// sent through JWT validation, adding latency on every request.
pub fn looks_like_jwt(credential: &str) -> bool {
    if credential.len() <= 32 {
        return false;
    }
    let parts: Vec<&str> = credential.splitn(4, '.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}
```

- [ ] **Step 2: Check for existing tests and add coverage**

In `crates/proxy/src/server/oidc.rs`, find the test module (or add one) and ensure these cases are covered:

```rust
#[cfg(test)]
mod jwt_heuristic_tests {
    use super::looks_like_jwt;

    #[test]
    fn real_jwt_segments_accepted() {
        // header.payload.signature — all Base64url chars
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1c2VyMSIsImlzcyI6Imh0dHBzOi8vaWRwIn0.AAAA";
        assert!(looks_like_jwt(jwt));
    }

    #[test]
    fn api_key_with_dots_rejected() {
        // API key that happens to have two dots
        assert!(!looks_like_jwt("sk-abc.def.ghi0123456789abcdef"));
    }

    #[test]
    fn non_base64url_chars_rejected() {
        // Contains '+' and '/' which are Base64 but not Base64url
        assert!(!looks_like_jwt("abc+def.ghi/jkl.mno+pqr"));
    }

    #[test]
    fn short_credential_rejected() {
        assert!(!looks_like_jwt("a.b.c"));
    }

    #[test]
    fn two_dots_only_rejected() {
        assert!(!looks_like_jwt("not.a.jwt.at.all.really.long.string.here.yes"));
    }

    #[test]
    fn empty_segment_rejected() {
        assert!(!looks_like_jwt("abc..xyz012345678901234567890123456789"));
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p anyllm_proxy jwt_heuristic 2>&1
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all jwt_heuristic_tests pass, no regressions.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/server/oidc.rs
git commit -m "fix(security): tighten looks_like_jwt to require valid Base64url segments"
```

---

## Task 8: Assert single-threaded invariant before unsafe set_var (Finding 7, LOW)

**Files:**
- Modify: `crates/proxy/src/main.rs`

The three `unsafe { set_var }` calls are safe today but fragile: if a future refactor adds a `tokio::spawn` before them, the invariant breaks silently. Add a `debug_assert!` that fires in debug builds if the invariant is violated.

- [ ] **Step 1: Add debug_assert before each unsafe set_var block**

In `crates/proxy/src/main.rs`, there are three unsafe blocks that call `set_var`:

**Block 1** (around line 24 — env_file_vars loop):

Old:
```rust
    // SAFETY: single-threaded, before tokio spawns workers.
    unsafe {
        for (key, val) in &env_file_vars {
            std::env::set_var(key, val);
        }
    }
```

New:
```rust
    // SAFETY: single-threaded, before tokio spawns workers.
    // The debug_assert catches future regressions if a spawn is added before this call.
    debug_assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "set_var called after tokio runtime started; this is unsound in a multi-threaded context"
    );
    unsafe {
        for (key, val) in &env_file_vars {
            std::env::set_var(key, val);
        }
    }
```

**Block 2** (around line 63 — aliases loop):

Old:
```rust
    // Apply alias overrides so config::MultiConfig::load() sees them.
    // SAFETY: still single-threaded at this point (no spawns yet).
    unsafe {
        for (key, val) in &aliases {
            std::env::set_var(key, val);
        }
    }
```

New:
```rust
    // Apply alias overrides so config::MultiConfig::load() sees them.
    // SAFETY: still single-threaded at this point (no spawns yet).
    debug_assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "set_var called after tokio runtime started; this is unsound in a multi-threaded context"
    );
    unsafe {
        for (key, val) in &aliases {
            std::env::set_var(key, val);
        }
    }
```

**Block 3** (around line 77 — litellm_master_key):

Old:
```rust
            // SAFETY: still single-threaded, no spawns yet.
            unsafe { std::env::set_var("PROXY_API_KEYS", mk) };
```

New:
```rust
            // SAFETY: still single-threaded, no spawns yet.
            debug_assert!(
                tokio::runtime::Handle::try_current().is_err(),
                "set_var called after tokio runtime started; this is unsound in a multi-threaded context"
            );
            unsafe { std::env::set_var("PROXY_API_KEYS", mk) };
```

- [ ] **Step 2: Build and test**

```bash
cargo build -p anyllm_proxy 2>&1 | tail -20
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: clean build, no regressions. The `debug_assert!` executes in test mode; since these calls happen before `tokio::main` spawns workers, `try_current().is_err()` will be true and the assertion passes.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "fix(security): assert single-threaded invariant before unsafe set_var calls"
```

---

## Final verification

- [ ] **Full build and test run**

```bash
cargo build 2>&1 | tail -10
cargo test 2>&1 | tail -20
cargo clippy -- -D warnings 2>&1 | tail -30
```

Expected: clean build, ~906+ tests passing (same as before plus new tests), no clippy warnings.
