# Security Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Address 21 security findings from the anyllm-proxy security audit, ordered by severity.

**Architecture:** Incremental hardening, one concern per task. Each task is independently shippable. No structural refactors, no new crates. Where a fix requires a new dependency, it is called out explicitly for approval.

**Tech Stack:** Rust stable (1.83+, edition 2021), axum 0.8, sha2 0.10, hmac 0.12, subtle 2, rusqlite 0.32

---

## File Map

| File | Changes |
|------|---------|
| `crates/proxy/Cargo.toml` | Add `hmac` dep, add `zeroize` dep |
| `crates/proxy/src/config/env_aliases.rs` | Return `Vec<(String,String)>` instead of calling `set_var` |
| `crates/proxy/src/config/litellm.rs` | Return master_key instead of calling `set_var` |
| `crates/proxy/src/main.rs` | Consume alias/litellm returns, set vars in one place; add TLS warning |
| `crates/proxy/src/admin/keys.rs` | HMAC-SHA256 keyed hashing with server secret |
| `crates/proxy/src/admin/db.rs` | Add `hmac_secret` to admin DB, migration for re-hashing; add body limit; enum for status filter |
| `crates/proxy/src/admin/auth.rs` | Configurable token path via `ADMIN_TOKEN_PATH` env var |
| `crates/proxy/src/server/middleware.rs` | AUTH_MODE enforcement (keys-only, oidc-only, both) |
| `crates/proxy/src/callbacks.rs` | Warn on HTTP (non-HTTPS) webhook URLs |
| `crates/proxy/src/ratelimit.rs` | Add `RATE_LIMIT_FAIL_POLICY` (open/closed) |
| `crates/proxy/src/backend/bedrock_client.rs` | Zeroize credentials on drop |
| `crates/proxy/src/server/routes.rs` | Add body limit to admin routes; optional CORS layer |
| `crates/proxy/admin-ui/index.html` | CSP header, sessionStorage, nonce for inline script |
| `crates/proxy/src/admin/routes.rs` | Rate limiting on admin endpoints |
| `crates/proxy/src/admin/state.rs` | Audit logging for config changes |
| `crates/proxy/src/lib.rs` | No changes |

---

### Task 1: Consolidate `set_var` calls (Critical #1)

**Why:** `std::env::set_var` is unsound in multi-threaded contexts. Currently scattered across 3 files. Consolidate into `main()` before tokio starts, and refactor helpers to return values instead of mutating the environment.

**Files:**
- Modify: `crates/proxy/src/config/env_aliases.rs`
- Modify: `crates/proxy/src/config/litellm.rs`
- Modify: `crates/proxy/src/main.rs`

- [ ] **Step 1: Refactor `apply_env_aliases` to return overrides**

Change `apply_env_aliases()` to return a `Vec<(&'static str, String)>` of (target_var, value) pairs instead of calling `set_var` internally.

```rust
// crates/proxy/src/config/env_aliases.rs

/// Compute env var overrides from LiteLLM aliases. Does NOT mutate the environment.
/// Caller is responsible for applying returned overrides via set_var.
pub fn compute_env_aliases() -> Vec<(&'static str, String)> {
    let mut overrides = Vec::new();
    for &(from, to) in ALIASES {
        if std::env::var(to).is_err() {
            if let Ok(val) = std::env::var(from) {
                overrides.push((to, val));
                tracing::debug!(from = %from, to = %to, "computed LiteLLM env var alias");
            }
        }
    }
    overrides
}
```

Keep `apply_env_aliases()` as a thin wrapper calling `compute_env_aliases()` + `set_var` for backward compat in tests, but mark it `#[deprecated]`.

- [ ] **Step 2: Refactor LiteLLM config to return master_key**

In `crates/proxy/src/config/litellm.rs`, change the `master_key` handling in `parse_litellm_config` (around line 178) to return the key value instead of calling `set_var`. Add it to the return type or a new struct field.

```rust
// Instead of:
//   unsafe { std::env::set_var("PROXY_API_KEYS", &resolved) };
// Return it:
pub struct LitellmParseResult {
    pub backends: Vec<BackendConfig>,
    pub master_key: Option<String>,
    // ... existing fields
}
```

- [ ] **Step 3: Apply all `set_var` calls in one block in `main()`**

In `main.rs`, after calling `compute_env_aliases()` and parsing litellm config, apply all overrides in a single `unsafe` block with a clear comment:

```rust
// main.rs, before tokio::main or any thread spawning
fn apply_env_overrides(aliases: Vec<(&str, String)>, litellm_master_key: Option<String>) {
    // SAFETY: Called exactly once, single-threaded, before tokio runtime starts.
    // All set_var calls are consolidated here.
    unsafe {
        for (key, val) in aliases {
            std::env::set_var(key, &val);
        }
        if let Some(mk) = litellm_master_key {
            if std::env::var("PROXY_API_KEYS").is_err() {
                std::env::set_var("PROXY_API_KEYS", &mk);
            }
        }
    }
}
```

- [ ] **Step 4: Update tests**

Update the two existing tests in `env_aliases.rs` to use `compute_env_aliases()` and verify return values rather than side effects on environment.

```rust
#[test]
fn alias_computed_when_target_unset() {
    let _lock = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::remove_var("PROXY_API_KEYS");
        std::env::set_var("LITELLM_MASTER_KEY", "sk-test-master");
    }
    let overrides = compute_env_aliases();
    assert!(overrides.iter().any(|(k, v)| *k == "PROXY_API_KEYS" && v == "sk-test-master"));
    unsafe {
        std::env::remove_var("LITELLM_MASTER_KEY");
    }
}
```

- [ ] **Step 5: Run tests and verify**

Run: `cargo test -p anyllm_proxy env_alias`
Expected: All alias tests pass.

Run: `cargo clippy -p anyllm_proxy -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/config/env_aliases.rs crates/proxy/src/config/litellm.rs crates/proxy/src/main.rs
git commit -m "security: consolidate set_var calls to single pre-runtime block"
```

---

### Task 2: HMAC-SHA256 keyed hashing for virtual keys (Critical #3)

**Why:** Plain SHA-256 without salt means identical keys produce identical hashes across installations. While `sk-vk{uuid}{uuid}` keys have 244 bits of entropy (making rainbow tables impractical), defense-in-depth calls for keyed hashing. HMAC-SHA256 with a per-installation secret eliminates cross-installation hash correlation.

**Requires new dependency:** `hmac = "0.12"` (ask user for approval before proceeding).

**Files:**
- Modify: `crates/proxy/Cargo.toml` (add `hmac = "0.12"`)
- Modify: `crates/proxy/src/admin/keys.rs`
- Modify: `crates/proxy/src/admin/db.rs` (store/retrieve HMAC secret, re-hash migration)
- Modify: `crates/proxy/src/main.rs` (pass HMAC secret at startup)

- [ ] **Step 1: Write failing test for HMAC-keyed hashing**

```rust
// In crates/proxy/src/admin/keys.rs, add to #[cfg(test)] mod tests
#[test]
fn hmac_hash_differs_from_plain_sha256() {
    let key = "sk-vktest1234";
    let secret = b"install-secret-abc";
    let hmac_hash = hmac_hash_key(key, secret);
    let plain_hash = hash_key(key);
    assert_ne!(hmac_hash, plain_hash, "HMAC hash must differ from plain SHA-256");
}

#[test]
fn hmac_hash_differs_with_different_secrets() {
    let key = "sk-vktest1234";
    let h1 = hmac_hash_key(key, b"secret-a");
    let h2 = hmac_hash_key(key, b"secret-b");
    assert_ne!(h1, h2, "different secrets must produce different hashes");
}

#[test]
fn hmac_hash_deterministic_with_same_secret() {
    let key = "sk-vktest1234";
    let secret = b"consistent-secret";
    assert_eq!(hmac_hash_key(key, secret), hmac_hash_key(key, secret));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p anyllm_proxy hmac_hash`
Expected: FAIL, `hmac_hash_key` not found.

- [ ] **Step 3: Implement `hmac_hash_key`**

```rust
// crates/proxy/src/admin/keys.rs
use hmac::{Hmac, Mac};
type HmacSha256 = Hmac<sha2::Sha256>;

/// HMAC-SHA256 hash a key with a per-installation secret. Returns hex-encoded result.
/// Use this for all new key hashing. Falls back to plain SHA-256 only during migration.
pub fn hmac_hash_key(key: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC accepts any key length");
    mac.update(key.as_bytes());
    let result = mac.finalize();
    bytes_to_hex(&result.into_bytes())
}
```

Update `generate_virtual_key` to accept a secret parameter:

```rust
pub fn generate_virtual_key(hmac_secret: &[u8]) -> (String, String, String) {
    let a = uuid::Uuid::new_v4().as_simple().to_string();
    let b = uuid::Uuid::new_v4().as_simple().to_string();
    let raw_key = format!("sk-vk{}{}", a, b);
    let key_prefix = raw_key[..8].to_string();
    let key_hash_hex = hmac_hash_key(&raw_key, hmac_secret);
    (raw_key, key_prefix, key_hash_hex)
}
```

- [ ] **Step 4: Add HMAC secret storage to DB**

In `crates/proxy/src/admin/db.rs`, add a `settings` table to store the HMAC secret:

```rust
pub fn ensure_hmac_secret(conn: &rusqlite::Connection) -> Vec<u8> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL);"
    ).expect("create settings table");

    let existing: Option<Vec<u8>> = conn
        .query_row("SELECT value FROM settings WHERE key = 'hmac_secret'", [], |row| row.get(0))
        .ok();

    if let Some(secret) = existing {
        return secret;
    }

    // Generate 32-byte random secret
    let secret: [u8; 32] = {
        let mut buf = [0u8; 32];
        // Use uuid v4 as entropy source (available in deps) -- two UUIDs = 32 bytes
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        buf[..16].copy_from_slice(a.as_bytes());
        buf[16..].copy_from_slice(b.as_bytes());
        buf
    };

    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('hmac_secret', ?1)",
        [&secret[..]],
    ).expect("insert hmac_secret");

    secret.to_vec()
}
```

- [ ] **Step 5: Add migration to re-hash existing keys**

```rust
/// Re-hash all virtual keys from plain SHA-256 to HMAC-SHA256.
/// Called once at startup if migration hasn't been performed.
pub fn migrate_key_hashes(conn: &rusqlite::Connection, hmac_secret: &[u8]) -> rusqlite::Result<usize> {
    // Check if migration already done
    let migrated: bool = conn
        .query_row("SELECT value FROM settings WHERE key = 'hmac_migration_done'", [], |row| {
            let v: String = row.get(0)?;
            Ok(v == "true")
        })
        .unwrap_or(false);

    if migrated {
        return Ok(0);
    }

    // Migration requires the raw keys -- but we don't have them.
    // Instead, newly created keys use HMAC. Existing keys are verified
    // by trying BOTH the HMAC hash and the legacy SHA-256 hash during auth.
    // Mark migration as "dual-mode" so we know to check both.
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('hmac_migration_done', 'dual')",
        [],
    )?;

    Ok(0) // No keys actually re-hashed; dual-mode lookup handles it
}
```

- [ ] **Step 6: Update middleware auth to check both HMAC and legacy hashes**

In `crates/proxy/src/server/middleware.rs`, when comparing virtual key hashes, compute both the HMAC hash and the legacy SHA-256 hash of the credential and check against the DashMap:

```rust
// In validate_auth, virtual key lookup section:
let credential_hmac_hash = crate::admin::keys::hmac_hash_key(&credential, &hmac_secret);
let credential_legacy_hash = crate::admin::keys::hash_key(&credential);

// Try HMAC hash first (new keys), fall back to legacy (pre-migration keys)
let vk_meta = map.get_mut(&credential_hmac_hash)
    .or_else(|| map.get_mut(&credential_legacy_hash));
```

This requires passing the HMAC secret through to middleware (add to app state).

- [ ] **Step 7: Run all tests**

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass, including virtual key CRUD integration tests.

- [ ] **Step 8: Commit**

```bash
git add crates/proxy/Cargo.toml crates/proxy/src/admin/keys.rs crates/proxy/src/admin/db.rs crates/proxy/src/server/middleware.rs crates/proxy/src/main.rs
git commit -m "security: HMAC-SHA256 keyed hashing for virtual keys with dual-mode migration"
```

---

### Task 3: Configurable admin token file path (Critical #2)

**Why:** Default `.admin_token` in CWD may be on a shared volume in containers. Allow operators to specify a secure path.

**Files:**
- Modify: `crates/proxy/src/main.rs` (read `ADMIN_TOKEN_PATH` env var)

- [ ] **Step 1: Write test for configurable path**

```rust
#[test]
fn admin_token_path_from_env() {
    let path = resolve_admin_token_path();
    // Default: .admin_token in CWD
    assert!(path.ends_with(".admin_token"));
}
```

- [ ] **Step 2: Implement configurable path**

In `main.rs`, replace the hardcoded `.admin_token` path:

```rust
fn resolve_admin_token_path() -> std::path::PathBuf {
    match std::env::var("ADMIN_TOKEN_PATH") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => std::path::PathBuf::from(".admin_token"),
    }
}
```

Use this wherever the token file path is referenced.

- [ ] **Step 3: Add startup warning on non-Unix platforms**

Where the token file is written, add a warning if not Unix:

```rust
#[cfg(not(unix))]
tracing::warn!(
    path = %token_path.display(),
    "admin token file written without restrictive permissions (non-Unix platform); \
     secure this file manually or set ADMIN_TOKEN_PATH to a protected location"
);
```

- [ ] **Step 4: Run tests and verify**

Run: `cargo test -p anyllm_proxy admin_token`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "security: configurable admin token path via ADMIN_TOKEN_PATH"
```

---

### Task 4: Enforce AUTH_MODE for OIDC/key separation (High #4)

**Why:** When OIDC is configured, failed JWT validation silently falls through to key-based auth. This should be configurable so operators can enforce OIDC-only auth.

**Files:**
- Modify: `crates/proxy/src/server/middleware.rs`
- Modify: `crates/proxy/src/main.rs` (pass AUTH_MODE from env)

- [ ] **Step 1: Write failing tests for AUTH_MODE**

```rust
#[cfg(test)]
mod auth_mode_tests {
    use super::*;

    #[test]
    fn auth_mode_oidc_only_rejects_api_keys() {
        let mode = AuthMode::OidcOnly;
        let credential = "sk-some-api-key";
        assert!(!mode.allows_key_auth());
        assert!(mode.allows_oidc());
    }

    #[test]
    fn auth_mode_keys_only_rejects_jwts() {
        let mode = AuthMode::KeysOnly;
        assert!(mode.allows_key_auth());
        assert!(!mode.allows_oidc());
    }

    #[test]
    fn auth_mode_both_allows_fallthrough() {
        let mode = AuthMode::Both;
        assert!(mode.allows_key_auth());
        assert!(mode.allows_oidc());
    }
}
```

- [ ] **Step 2: Implement AuthMode enum**

```rust
// crates/proxy/src/server/middleware.rs

/// Controls which authentication methods are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Only static/virtual API keys accepted (default when no OIDC configured).
    KeysOnly,
    /// Only OIDC JWT tokens accepted. API keys are rejected.
    OidcOnly,
    /// Both methods accepted. Failed JWT falls through to key check (current behavior).
    Both,
}

impl AuthMode {
    pub fn from_env() -> Self {
        match std::env::var("AUTH_MODE").as_deref() {
            Ok("oidc") | Ok("oidc-only") => AuthMode::OidcOnly,
            Ok("keys") | Ok("keys-only") => AuthMode::KeysOnly,
            Ok("both") => AuthMode::Both,
            _ => AuthMode::Both, // backward-compatible default
        }
    }

    pub fn allows_key_auth(&self) -> bool {
        matches!(self, AuthMode::KeysOnly | AuthMode::Both)
    }

    pub fn allows_oidc(&self) -> bool {
        matches!(self, AuthMode::OidcOnly | AuthMode::Both)
    }
}
```

- [ ] **Step 3: Wire AUTH_MODE into `validate_auth`**

In the auth validation logic, check the mode before attempting each auth method:

```rust
// In validate_auth:
if auth_mode.allows_oidc() && looks_like_jwt(&credential) {
    if let Some(oidc) = &oidc_config {
        match oidc.validate_token(&credential).await {
            Ok(claims) => return Ok(/* authenticated via OIDC */),
            Err(e) => {
                if auth_mode == AuthMode::OidcOnly {
                    // Don't fall through -- OIDC is the only allowed method
                    return Err(/* 401 invalid token */);
                }
                // AuthMode::Both -- fall through to key check
                tracing::debug!("JWT validation failed, trying key auth: {e}");
            }
        }
    }
}

if !auth_mode.allows_key_auth() {
    return Err(/* 401: key-based auth not enabled */);
}
// ... existing key check logic
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p anyllm_proxy auth_mode`
Expected: PASS.

Run: `cargo test -p anyllm_proxy validate_auth`
Expected: PASS (existing auth tests still work with default `Both` mode).

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/middleware.rs crates/proxy/src/main.rs
git commit -m "security: add AUTH_MODE to control OIDC/key auth fallthrough"
```

---

### Task 5: Harden SQL status filter (High #5)

**Why:** Current status filter parsing is safe but the pattern invites injection if extended. Convert to an enum with exhaustive matching.

**Files:**
- Modify: `crates/proxy/src/admin/db.rs`

- [ ] **Step 1: Write test for status filter enum**

```rust
#[test]
fn status_filter_rejects_invalid_input() {
    assert!(StatusFilter::parse("200").is_some());
    assert!(StatusFilter::parse("2xx").is_some());
    assert!(StatusFilter::parse("4xx").is_some());
    assert!(StatusFilter::parse("5xx").is_some());
    assert!(StatusFilter::parse("999").is_some()); // valid exact code
    assert!(StatusFilter::parse("abc").is_none());
    assert!(StatusFilter::parse("2xx; DROP TABLE").is_none());
    assert!(StatusFilter::parse("").is_none());
}
```

- [ ] **Step 2: Implement StatusFilter enum**

```rust
/// Typed status code filter -- prevents any possibility of SQL injection.
pub enum StatusFilter {
    /// Exact HTTP status code (e.g., 200, 404).
    Exact(u16),
    /// Status class wildcard (2xx, 4xx, 5xx).
    Class2xx,
    Class4xx,
    Class5xx,
}

impl StatusFilter {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "2xx" => Some(Self::Class2xx),
            "4xx" => Some(Self::Class4xx),
            "5xx" => Some(Self::Class5xx),
            other => other.parse::<u16>().ok().map(Self::Exact),
        }
    }

    /// Append the appropriate WHERE clause and parameters.
    pub fn apply_to_query(&self, sql: &mut String, params: &mut Vec<Box<dyn rusqlite::types::ToSql>>) {
        match self {
            Self::Exact(code) => {
                sql.push_str(" AND status_code = ?");
                params.push(Box::new(*code as i64));
            }
            Self::Class2xx => sql.push_str(" AND status_code >= 200 AND status_code < 300"),
            Self::Class4xx => sql.push_str(" AND status_code >= 400 AND status_code < 500"),
            Self::Class5xx => sql.push_str(" AND status_code >= 500 AND status_code < 600"),
        }
    }
}
```

- [ ] **Step 3: Replace raw string matching in `query_request_log`**

Replace the existing status filter parsing with:

```rust
if let Some(status_str) = &filter.status {
    if let Some(sf) = StatusFilter::parse(status_str) {
        sf.apply_to_query(&mut sql, &mut param_values);
    }
    // Invalid filter silently ignored (no results is safer than error)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p anyllm_proxy status_filter`
Expected: PASS.

Run: `cargo test -p anyllm_proxy query_request_log`
Expected: PASS (existing tests unaffected).

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/admin/db.rs
git commit -m "security: typed StatusFilter enum to prevent SQL injection by construction"
```

---

### Task 6: Warn on HTTP webhook URLs (High #7)

**Why:** Sending request metadata over plaintext HTTP leaks operational data.

**Files:**
- Modify: `crates/proxy/src/callbacks.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn http_webhook_produces_warning() {
    // CallbackConfig::new should accept HTTP URLs but log a warning.
    // We verify the URL is kept (not rejected) since this is a warning, not enforcement.
    let config = CallbackConfig::new(vec!["http://example.com/hook".into()]);
    assert!(config.is_some());
}

#[test]
fn https_webhook_no_warning() {
    let config = CallbackConfig::new(vec!["https://example.com/hook".into()]);
    assert!(config.is_some());
}
```

- [ ] **Step 2: Add warning for HTTP URLs**

In `CallbackConfig::new`, after the protocol filter, add:

```rust
for url in &valid_urls {
    if url.starts_with("http://") && !url.starts_with("http://localhost") && !url.starts_with("http://127.0.0.1") {
        tracing::warn!(
            url = %url,
            "webhook URL uses plaintext HTTP; request metadata (model, tokens, latency) \
             will be sent unencrypted. Use HTTPS in production."
        );
    }
}
```

Allow localhost HTTP without warning (common in dev).

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_proxy callback`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/callbacks.rs
git commit -m "security: warn on plaintext HTTP webhook URLs"
```

---

### Task 7: Configurable Redis rate limit fail policy (Medium #12)

**Why:** Redis failure currently allows all requests (fail-open). Operators should be able to choose fail-closed for security-sensitive deployments.

**Files:**
- Modify: `crates/proxy/src/ratelimit.rs`
- Modify: `crates/proxy/src/main.rs` (pass policy from env)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn fail_policy_from_env_defaults_to_open() {
    let policy = RateLimitFailPolicy::from_env_val(None);
    assert_eq!(policy, RateLimitFailPolicy::Open);
}

#[test]
fn fail_policy_closed_from_env() {
    let policy = RateLimitFailPolicy::from_env_val(Some("closed"));
    assert_eq!(policy, RateLimitFailPolicy::Closed);
}
```

- [ ] **Step 2: Implement RateLimitFailPolicy**

```rust
// crates/proxy/src/ratelimit.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitFailPolicy {
    /// Allow requests when Redis is unavailable (current behavior, default).
    Open,
    /// Reject requests when Redis is unavailable (return 503).
    Closed,
}

impl RateLimitFailPolicy {
    pub fn from_env_val(val: Option<&str>) -> Self {
        match val {
            Some("closed") | Some("deny") => Self::Closed,
            _ => Self::Open,
        }
    }
}
```

- [ ] **Step 3: Wire policy into check_rpm/check_tpm**

```rust
// In check_rpm:
Err(e) => {
    tracing::warn!(error = %e, "Redis RPM check failed");
    match self.fail_policy {
        RateLimitFailPolicy::Open => Ok(()),
        RateLimitFailPolicy::Closed => {
            tracing::error!("rate limit fail-closed: rejecting request due to Redis failure");
            Err(60) // retry after 60s
        }
    }
}
```

Add `fail_policy: RateLimitFailPolicy` field to `RedisRateLimiter`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p anyllm_proxy rate_limit_fail`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/ratelimit.rs crates/proxy/src/main.rs
git commit -m "security: configurable rate limit fail policy (open/closed) via RATE_LIMIT_FAIL_POLICY"
```

---

### Task 8: Zeroize Bedrock credentials on drop (Medium #11)

**Why:** AWS credentials in memory appear in heap dumps and core files.

**Requires new dependency:** `zeroize = "1"` (ask user for approval).

**Files:**
- Modify: `crates/proxy/Cargo.toml` (add `zeroize = "1"`)
- Modify: `crates/proxy/src/backend/bedrock_client.rs`

- [ ] **Step 1: Add zeroize dependency**

Add to `[dependencies]` in `crates/proxy/Cargo.toml`:
```toml
zeroize = "1"
```

- [ ] **Step 2: Wrap credentials with Zeroizing**

```rust
// crates/proxy/src/backend/bedrock_client.rs
use zeroize::Zeroizing;

pub struct BedrockClient {
    // Wrap the credentials so they're zeroed on drop
    credentials: Zeroizing<aws_credential_types::Credentials>,
    // ... rest unchanged
}
```

The `Zeroizing<T>` wrapper calls `zeroize()` on the inner value when dropped. Since `aws_credential_types::Credentials` contains `String` fields internally, this zeros the heap allocations.

Note: `aws_credential_types::Credentials` may not implement `Zeroize`. If it doesn't, wrap the individual secret key string instead:

```rust
pub struct BedrockClient {
    access_key_id: String,
    secret_access_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
    region: String,
    // ... rest
}
```

And construct `Credentials` on each sign operation from these fields.

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_proxy bedrock`
Expected: PASS (compile check, Bedrock tests are `#[ignore]`).

Run: `cargo clippy -p anyllm_proxy -- -D warnings`
Expected: No warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/Cargo.toml crates/proxy/src/backend/bedrock_client.rs
git commit -m "security: zeroize Bedrock credentials on drop"
```

---

### Task 9: Rate limiting on admin API endpoints (Medium #9)

**Why:** Admin API has no rate limiting. Defense-in-depth against token brute-force.

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs`

- [ ] **Step 1: Add a simple IP-based rate limiter for admin endpoints**

Use a `DashMap<IpAddr, (u64, u32)>` (timestamp window start, count) for a basic sliding window. 10 requests per minute per IP.

```rust
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::LazyLock;

static ADMIN_RATE_LIMIT: LazyLock<DashMap<IpAddr, (u64, u32)>> = LazyLock::new(DashMap::new);
const ADMIN_RPM: u32 = 10;

/// Check admin rate limit. Returns true if allowed.
fn check_admin_rate_limit(ip: IpAddr) -> bool {
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
    } else if *count < ADMIN_RPM {
        *count += 1;
        true
    } else {
        false
    }
}
```

- [ ] **Step 2: Apply rate limit middleware to admin routes**

Add a middleware layer to the admin router that calls `check_admin_rate_limit` and returns 429 when exceeded:

```rust
async fn admin_rate_limit_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let ip = req
        .extensions()
        .get::<std::net::SocketAddr>()
        .map(|a| a.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    if !check_admin_rate_limit(ip) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "admin rate limit exceeded",
        ).into_response();
    }
    next.run(req).await
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_proxy admin`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "security: add rate limiting to admin API endpoints (10 RPM per IP)"
```

---

### Task 10: Audit logging for config changes (Medium #10)

**Why:** Model mapping changes via admin API have no audit trail. An attacker with admin access could redirect traffic without detection.

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs` or `crates/proxy/src/admin/state.rs`

- [ ] **Step 1: Add structured logging to config update handler**

In the `PUT /admin/api/config` handler, log before and after values:

```rust
tracing::info!(
    key = %key,
    old_value = %old_value,
    new_value = %new_value,
    source_ip = %client_ip,
    "admin config change"
);
```

- [ ] **Step 2: Validate model names to prevent path traversal**

```rust
fn is_safe_model_name(name: &str) -> bool {
    // Model names should be alphanumeric with hyphens, dots, slashes (for provider/model format)
    // Reject anything with .., control chars, or query strings
    !name.contains("..")
        && !name.contains('?')
        && !name.contains('#')
        && name.chars().all(|c| c.is_alphanumeric() || "-_./: ".contains(c))
}
```

Reject config updates with invalid model names with 400.

- [ ] **Step 3: Run tests**

Run: `cargo test -p anyllm_proxy config`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/admin/routes.rs crates/proxy/src/admin/state.rs
git commit -m "security: audit log config changes and validate model names"
```

---

### Task 11: Admin UI security hardening (Low #15, #16)

**Why:** Inline scripts with no CSP, and admin token in localStorage (persists across sessions, XSS-exfiltrable).

**Files:**
- Modify: `crates/proxy/admin-ui/index.html`
- Modify: `crates/proxy/src/admin/routes.rs` (add CSP response header)

- [ ] **Step 1: Switch localStorage to sessionStorage**

In `admin-ui/index.html`, replace all `localStorage` references:

```javascript
// Before:
var TOKEN = localStorage.getItem('admin_token');
// ...
localStorage.setItem('admin_token', TOKEN);

// After:
var TOKEN = sessionStorage.getItem('admin_token');
// ...
sessionStorage.setItem('admin_token', TOKEN);
```

This ensures the token is cleared when the browser tab closes.

- [ ] **Step 2: Add CSP header to admin UI response**

In the admin routes where `index.html` is served, add a Content-Security-Policy header:

```rust
// When serving the admin UI HTML:
let csp = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self' ws: wss:; img-src 'self' data:; frame-ancestors 'none'";
headers.insert(
    axum::http::header::CONTENT_SECURITY_POLICY,
    csp.parse().unwrap(),
);
```

Note: `'unsafe-inline'` is required because the script is inline. A future improvement would extract the JS to a separate file, but that is out of scope for this task.

- [ ] **Step 3: Verify admin UI still works**

Manual check: Start proxy, open admin UI at localhost:3001, verify token prompt works and API calls succeed.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/admin-ui/index.html crates/proxy/src/admin/routes.rs
git commit -m "security: admin UI uses sessionStorage and CSP header"
```

---

### Task 12: Add body size limit to admin API (Informational #20)

**Why:** Admin API has no explicit body limit. Config and key endpoints don't need more than 1MB.

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs`

- [ ] **Step 1: Add DefaultBodyLimit to admin router**

```rust
use axum::extract::DefaultBodyLimit;

// In admin router construction:
.layer(DefaultBodyLimit::max(1_048_576)) // 1 MB
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p anyllm_proxy admin`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "security: add 1MB body size limit to admin API"
```

---

### Task 13: Startup warning for plaintext listener with API keys (Low #18)

**Why:** API keys transmitted in cleartext when bound to 0.0.0.0 without TLS.

**Files:**
- Modify: `crates/proxy/src/main.rs`

- [ ] **Step 1: Add warning after bind**

After the proxy listener binds, check conditions and warn:

```rust
if std::env::var("PROXY_API_KEYS").is_ok() || has_virtual_keys {
    let listen_addr = /* the bound address */;
    if !listen_addr.ip().is_loopback() {
        tracing::warn!(
            addr = %listen_addr,
            "proxy is listening on a non-loopback address without TLS; \
             API keys will be transmitted in cleartext. \
             Place a TLS-terminating reverse proxy in front of this service."
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo build -p anyllm_proxy`
Expected: Compiles clean.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/main.rs
git commit -m "security: warn at startup when serving API keys over plaintext HTTP"
```

---

### Task 14: Fix stale Redis comment (Low #13)

**Why:** Misleading comment could cause operators to skip Redis monitoring.

**Files:**
- Modify: wherever `CacheConfig.redis_url` has "Not yet used (placeholder)" comment

- [ ] **Step 1: Find and fix the comment**

Search for the stale comment and update it:

```rust
// Before:
/// Not yet used (placeholder).
pub redis_url: Option<String>,

// After:
/// Redis connection URL. Used for distributed rate limiting and L2 cache when the `redis` feature is enabled.
pub redis_url: Option<String>,
```

- [ ] **Step 2: Commit**

```bash
git add <file>
git commit -m "docs: fix stale redis_url placeholder comment"
```

---

### Task 15: Document informational findings as code comments (Informational #19, #21)

**Why:** Mutex poisoning recovery and batch file SQLite storage are deliberate design choices that should be documented for future maintainers.

**Files:**
- Modify: `crates/proxy/src/admin/db.rs` (add comments for mutex poisoning pattern and batch file storage)

- [ ] **Step 1: Add comments**

Near the `unwrap_or_else(|e| e.into_inner())` usages:

```rust
// Mutex poisoning recovery: if a prior request panicked while holding the lock,
// we recover the inner value rather than permanently locking the database.
// This is safe because SQLite transactions provide ACID guarantees -- a panic
// mid-transaction means the transaction was rolled back by SQLite.
let conn = db.lock().unwrap_or_else(|e| e.into_inner());
```

Near batch file storage:

```rust
// Note: batch file JSONL is stored directly in SQLite. For large batch files
// (>10MB), consider external blob storage. Current design prioritizes simplicity
// and single-binary deployment over storage efficiency.
```

- [ ] **Step 2: Commit**

```bash
git add crates/proxy/src/admin/db.rs
git commit -m "docs: document mutex poisoning recovery and batch storage tradeoffs"
```

---

## Issues NOT addressed in this plan (with rationale)

| # | Issue | Rationale |
|---|-------|-----------|
| 6 | SSRF DNS rebinding post-startup | The `SsrfSafeDnsResolver` in `http.rs` already handles this when the `ssrf-protection` feature is enabled. Making it default-on is a feature flag change that affects build deps. Worth doing but separate from security hardening. |
| 8 | Timing side-channel in DashMap lookup | The DashMap lookup is O(1) and the timing difference between "key exists" and "key missing" is sub-microsecond, indistinguishable over network. Not exploitable in practice. |
| 14 | No CORS headers | This is an API proxy, not a browser app. Adding CORS is a feature decision, not a security fix. If browser tooling needs it, add it then. |
| 17 | Unbounded streaming memory | Already bounded at 10MB per connection, 100 concurrent = 1GB max. This is a capacity planning concern, not a vulnerability. Document the math in a comment if desired. |

---

## Dependency Summary

New dependencies requiring approval:
1. **`hmac = "0.12"`** (Task 2) -- HMAC-SHA256 for virtual key hashing. Pure Rust, no transitive deps beyond `digest` (already pulled in by `sha2`).
2. **`zeroize = "1"`** (Task 8) -- Zero memory on drop for credentials. Pure Rust, no transitive deps.

Both are well-maintained, audited crates from the RustCrypto project.
