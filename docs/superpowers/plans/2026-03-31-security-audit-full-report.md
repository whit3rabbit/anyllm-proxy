# Security Audit Report: anyllm-proxy
**Date:** 2026-03-31
**Auditor:** Claude Code (claude-sonnet-4-6)
**Scope:** All Rust source in `crates/proxy/src`, `crates/translator/src`, `crates/client/src`, `crates/batch_engine/src`

---

## Executive Summary

The codebase demonstrates strong security awareness with multiple explicit mitigations in place: constant-time comparisons, HMAC-keyed virtual key hashing, fully parameterized SQL, SSRF-safe DNS resolution at connection time, CSRF double-submit with one-time server-tracked tokens, Origin/Host header validation against localhost, rate limiting on admin endpoints, and a compile-time feature gate for the dangerous bash tool. The security posture is meaningfully above baseline for a proxy of this complexity.

Several real issues remain, ranging from a critical RCE surface (when a non-default feature is enabled) to medium gaps in CSRF coverage and SSRF validation for a specific integration.

**Findings count:** 15 total (1 Critical, 3 High, 4 Medium, 4 Low, 3 Info)

---

## Findings

---

### FIND-01: Command Injection via `execute_bash` Tool

**Severity:** Critical (when `dangerous-builtin-tools` feature is compiled in and policy is `Allow`)
**Location:** `/crates/proxy/src/tools/builtin/bash.rs` lines 48-53

**Description:**
`BashTool::execute` passes the user-supplied `command` argument directly to `bash -c <command>` with no shell metacharacter filtering, no sandboxing (no chroot, no seccomp, no container), and no privilege drop. Any prompt injection that reaches the `execute_bash` tool with `Allow` policy can run arbitrary shell commands as the proxy process user.

The current mitigations are:
- Compile-time feature gate: `dangerous-builtin-tools` must be explicitly enabled.
- Default policy: `PassThrough` (tool call is returned to the client, not executed server-side).
- 30-second timeout and 256 KB output cap.

The 30-second timeout does not prevent data exfiltration via `curl`, lateral movement, or persistent backdoors. The 256 KB output cap does not prevent destructive operations.

**Impact:** Full server compromise if the feature is compiled in, the policy is set to `Allow`, and an adversarial prompt reaches the tool. In the default configuration (`PassThrough`), this is not exploitable.

**Recommendation:**
- Retain the feature gate and the `PassThrough` default. These are the correct mitigations.
- Add a hard startup `tracing::error!` or `panic!` when `execute_bash` is configured with `policy: allow` and the binary is not running in an explicitly sandboxed environment (e.g., check for a `ANYLLM_ALLOW_BASH_EXECUTION=1` env var as an additional acknowledgment step).
- Document prominently that `policy: allow` for `execute_bash` must never be used in multi-tenant or externally-accessible deployments without OS-level sandboxing (seccomp, namespace isolation, read-only filesystem).

---

### FIND-02: `unsafe std::env::set_var` Called After Tokio Runtime Is Active

**Severity:** High
**Location:** `/crates/proxy/src/main.rs` lines 24-27, 63-67, 77; `/crates/proxy/src/config/env_aliases.rs` lines 47, 57, 75, 83, 95, 104, 114

**Description:**
`std::env::set_var` is `unsafe` because the underlying POSIX `setenv(3)` is not thread-safe: it mutates the process-global environment without synchronization. Any concurrent thread that calls `getenv` during a `set_var` is a data race and undefined behavior (UB).

The inline safety comments claim "single-threaded, before tokio spawns workers." This is incorrect: `#[tokio::main]` with the default multi-thread flavor starts the runtime (and spawns worker threads) before the first user-visible line of `main` executes. The safety invariant the comments assert does not hold.

The risk is narrow in practice (the `set_var` calls happen very early, before most async work begins), but UB is UB. On some platforms and compiler versions, a concurrent `getenv` during `set_var` can produce a misread environment variable, meaning `PROXY_API_KEYS` or other critical security config could be silently skipped.

Note: the existing plan file `2026-03-31-security-audit-7-findings.md` (Task 8) addresses this by adding `debug_assert!(tokio::runtime::Handle::try_current().is_err(), ...)`. That is a detection improvement but does not fix the underlying race. The `#[tokio::main]` macro starts the runtime before user code, so `try_current()` will return `Ok` by the time any of these asserts fire -- making them always-true in practice. The assert as written will not catch the bug it is designed to catch.

**Impact:** Potential UB during startup. If `PROXY_API_KEYS` is read as garbage due to the race, auth may be silently misconfigured (open relay or rejection of valid keys).

**Recommendation:**
- Move all env file parsing and alias computation into a `sync main` that runs before `#[tokio::main]` is entered.
- Pattern: parse env file in a `fn main()`, collect a `Vec<(String,String)>`, call `set_var` in the sync context, then delegate to an `async fn run()` annotated with `#[tokio::main]`.
- This makes the SAFETY comment actually correct and eliminates UB.

---

### FIND-03: Admin Token Generated as UUID v4 (Low Entropy Format)

**Severity:** High (format concern) / Low (practical brute-force)
**Location:** `/crates/proxy/src/main.rs` line 440

**Description:**
The auto-generated admin token uses `uuid::Uuid::new_v4().to_string()`, which produces a 36-character hyphenated string like `550e8400-e29b-41d4-a716-446655440000`. UUID v4 provides 122 bits of entropy, which is adequate against blind brute-force. However:

1. The hyphenated format is a known, well-characterized character space (only hex digits and hyphens). Targeted brute-force tools can reduce their search space.
2. `uuid::Uuid::new_v4()` uses the `getrandom` crate internally (already imported), which is the correct CSPRNG source. This is not a weak RNG issue.
3. The admin rate limiter is 10 RPM per IP on `127.0.0.1`. Any local process can attempt ~14,400 guesses/day, which is still astronomically far from 2^122.

The practical risk is negligible. The concern is that using UUID as an admin credential is non-idiomatic and might give false confidence to operators who generate their own tokens following the same pattern.

**Impact:** Negligible brute-force risk at 122 bits. Format concern only.

**Recommendation:**
- Replace `uuid::Uuid::new_v4().to_string()` with direct `getrandom` output:
  ```rust
  let mut buf = [0u8; 32];
  getrandom::fill(&mut buf).expect("CSPRNG failed");
  hex::encode(buf)
  ```
  This gives 256 bits of entropy with no structure to exploit. `getrandom` and `hex` are already in `Cargo.toml`.

---

### FIND-04: Langfuse `LANGFUSE_HOST` Not SSRF-Validated

**Severity:** High
**Location:** `/crates/proxy/src/integrations/langfuse.rs` lines 29-31, 47-58

**Description:**
`LangfuseClient::from_env()` reads `LANGFUSE_HOST` from the environment and constructs an HTTP POST URL without any SSRF validation. The reqwest client is built with `reqwest::Client::builder()...build()`, which does not attach the `SsrfSafeDnsResolver` used by the rest of the codebase.

Every other outbound HTTP client in the codebase (OIDC discovery at `oidc.rs:92`, webhook callbacks at `callbacks.rs:72-77`, MCP servers at `main.rs:189`) uses either `validate_base_url()` or `build_http_client` with `ssrf_protection: true`. Langfuse is the only integration that skips both.

An operator who misconfigures `LANGFUSE_HOST=http://169.254.169.254` would unknowingly POST every request log entry (model name, token counts, latency, key_id, cost_usd) to the cloud metadata service.

**Impact:** SSRF to internal/cloud metadata services if `LANGFUSE_HOST` is misconfigured. Data exfiltration of request metadata. No authentication required at the Langfuse level -- the entire request log is sent to wherever `LANGFUSE_HOST` points.

**Recommendation:**
- In `LangfuseClient::from_env()`, call `crate::config::validate_base_url(&host)` and return `None` (with a `tracing::warn!`) if the URL is rejected.
- Replace the bare `reqwest::Client::builder()` with `build_http_client(&HttpClientConfig { ssrf_protection: true, ... })`.
- Reference: `callbacks.rs:45` does this correctly:
  ```rust
  if let Err(reason) = validate_base_url(u) {
      tracing::warn!(url = %u, reason = %reason, "ignoring webhook URL: SSRF risk");
      return false;
  }
  ```

---

### FIND-05: CSRF Token Cap Is Racy; Token Endpoint Not Rate-Limited

**Severity:** Medium
**Location:** `/crates/proxy/src/admin/routes.rs` lines 304-311, 336-399

**Description:**
Two related issues:

**5a (Race condition):** The CSRF issuance cap check uses a non-atomic check-then-insert:
```rust
if shared.issued_csrf_tokens.len() < MAX_ISSUED_CSRF_TOKENS {
    shared.issued_csrf_tokens.insert(token.clone(), ());
}
```
The comment acknowledges this is "inherently racy." Under concurrent requests, the map size can transiently exceed `MAX_ISSUED_CSRF_TOKENS` before the guard fires.

**5b (Rate limiting gap):** `/admin/csrf-token` is on the `public` router (lines 336-339), which does not have the `admin_rate_limit_middleware`. The rate limiter (10 RPM per IP) applies only to the `protected` router. An unauthenticated attacker can flood `GET /admin/csrf-token` to grow the `issued_csrf_tokens` map without hitting the 10 RPM cap. When the map is full, the silent drop means legitimate admin users receive tokens that always fail CSRF validation (the server never recorded them), locking out the admin UI.

**Impact:** Memory exhaustion DoS under sustained unauthenticated flooding of `GET /admin/csrf-token`. Legitimate admin lockout when the cap is full under attack.

**Recommendation:**
- Apply the admin rate limiter to `/admin/csrf-token` (move it to the `protected` router, or apply rate limiting separately to the public router).
- Replace the DashMap with a `moka::sync::Cache` with `max_capacity(1_000)` and `time_to_live(Duration::from_secs(86400))`. Moka's bounded cache provides a strict cap with automatic eviction, eliminating the race and the lockout scenario. `moka` is already in `Cargo.toml`. (This is Task 4 in the existing plan file.)

---

### FIND-06: Free-Text Query Params Not Length-Capped (Admin API)

**Severity:** Medium
**Location:** `/crates/proxy/src/admin/routes.rs` lines 1654-1656 (audit), 1015-1016 (requests); `/crates/proxy/src/admin/db.rs` lines 232-234, 1124-1130

**Description:**
The `backend`, `action`, and `target_type` filter parameters accepted by `GET /admin/api/requests`, `GET /admin/api/observability/overview`, and `GET /admin/api/audit` are passed to parameterized SQL (`AND backend = ?`, `AND action = ?`, etc.) without any length validation. The SQL itself is injection-safe due to parameterization.

However, query parameters are not subject to the 1 MB `DefaultBodyLimit`. A request with a 10 MB `backend` query string is accepted, allocated on the heap, and passed through to SQLite. Under the 10 RPM per-IP admin rate limit, this is low-severity DoS, but the cap is soft (the rate limiter uses a VecDeque, not a strict token bucket).

**Impact:** Low-severity admin-only memory pressure DoS from oversized query parameters.

**Recommendation:**
- Add a length cap (e.g., 128 bytes) to `backend`, `action`, and `target_type` query parameters before using them:
  ```rust
  let backend = params.backend.filter(|s| s.len() <= 128 && !s.is_empty());
  ```
- This is a defensive measure; the SQL injection risk is already fully mitigated.

---

### FIND-07: CSRF Middleware Does Not Cover `PATCH` Method

**Severity:** Medium
**Location:** `/crates/proxy/src/admin/routes.rs` lines 237-240

**Description:**
The `validate_csrf` middleware checks `POST | PUT | DELETE` but omits `PATCH`:
```rust
if matches!(
    method,
    axum::http::Method::POST | axum::http::Method::PUT | axum::http::Method::DELETE
) {
```

No current admin route uses `PATCH`, so this is not currently exploitable. It is a "gap by convention" that creates a latent vulnerability: if a `PATCH` route is added in the future (e.g., partial key update), it will silently bypass CSRF protection without any compiler warning or test failure.

**Impact:** Currently non-exploitable. Future PATCH routes would bypass CSRF.

**Recommendation:**
- Add `axum::http::Method::PATCH` to the `matches!` guard. One-line fix that closes the gap proactively.

---

### FIND-08: `read_file` Tool Allows Unrestricted File Read When `allowed_dirs` Is Empty

**Severity:** Low (feature-gated behind `dangerous-builtin-tools`)
**Location:** `/crates/proxy/src/tools/builtin/read_file.rs` lines 72-78

**Description:**
When `ReadFileTool` is registered with `allowed_dirs = []` (empty), it reads any absolute path the LLM requests, bounded only by a 1 MB size cap. The code emits a `tracing::warn!` but continues with the read:
```rust
} else {
    tracing::warn!(...);
    // falls through to read
}
```

If an LLM is manipulated via prompt injection to call `read_file` with policy `Allow` and no `allowed_dirs` configured, it can read `/etc/shadow`, the `.admin_token` file, the SQLite database, TLS private keys, or any other file readable by the proxy process user.

The feature gate (`dangerous-builtin-tools`) and `PassThrough` default provide the primary mitigations.

**Impact:** Arbitrary file read as the proxy process user if `allowed_dirs` is empty and policy is `Allow`.

**Recommendation:**
- Return an error instead of proceeding when `allowed_dirs` is empty and the tool is invoked:
  ```rust
  } else {
      return Err(
          "read_file requires allowed_dirs to be configured; refusing unrestricted file access".to_string()
      );
  }
  ```
  If unrestricted reads are intentional, require the operator to explicitly set `allowed_dirs: ["/"]` to acknowledge the risk.

---

### FIND-09: `QDRANT_URL` and `REDIS_URL` Not SSRF-Validated

**Severity:** Low
**Location:** `/crates/proxy/src/cache/semantic.rs` line 32-40; `/crates/proxy/src/ratelimit.rs` lines 74-77

**Description:**
`QDRANT_URL` and `REDIS_URL` are consumed directly by their respective client constructors without going through `validate_base_url()`. An operator who sets `QDRANT_URL=http://169.254.169.254/` would cause the proxy to connect to the cloud metadata endpoint on first cache operation.

The risk is lower than FIND-04 because:
- These are operator-supplied configuration values (not end-user controlled).
- The Qdrant and Redis clients parse URLs with their own logic and would likely produce connection errors, not useful responses.
- The `ssrf-protection` feature gate means the `SsrfSafeDnsResolver` is not attached to these clients anyway.

**Impact:** SSRF to internal services on operator misconfiguration. Not end-user exploitable.

**Recommendation:**
- Call `validate_base_url()` on both env vars at startup and log a warning (or refuse to start) if the URL fails validation.

---

### FIND-10: Admin Token Stored as `Arc<String>` Without Zeroize on Drop

**Severity:** Low
**Location:** `/crates/proxy/src/main.rs` line 458; admin auth middleware throughout

**Description:**
The admin token is stored as `Arc<String>` and cloned into every request handler. Rust `String` does not implement `zeroize::Zeroize`, so the token value remains in heap memory until the allocator reuses the page. On process crash, OOM kill, or core dump, the token is visible in memory.

The `zeroize` crate is already in `Cargo.toml` (`zeroize = "1"`), so no new dependency is needed.

**Impact:** Token leakage from core dumps or memory forensics. Low severity because the admin server binds only to 127.0.0.1.

**Recommendation:**
- Wrap the admin token in `zeroize::Zeroizing<String>`:
  ```rust
  use zeroize::Zeroizing;
  let admin_token = Arc::new(Zeroizing::new(admin_token));
  ```
- Update the type signature in `validate_admin_token` and the router construction accordingly.

---

### FIND-11: Timestamp Params in Observability Endpoint Not Validated

**Severity:** Low
**Location:** `/crates/proxy/src/admin/routes.rs` lines 848-855 (ObservabilityQuery)

**Description:**
The `get_observability_overview` endpoint constructs `since` and `until` internally from a clamped `hours` integer (lines 862-873), so its SQL parameters are safe. However, the `ObservabilityQuery` struct declares `since: Option<String>` and `until: Option<String>` as unused fields. If these are ever wired up to be passed to the database query, they would bypass the `check_time_range` validation that `get_requests` and `get_audit_log` apply.

This is a latent risk rather than a current vulnerability.

**Impact:** If wired up without validation, would pass unvalidated strings to SQLite timestamp comparisons (parameterized, so no SQL injection, but no index hint either).

**Recommendation:**
- Remove the `since`/`until` fields from `ObservabilityQuery` if they are not intended to be used, to prevent accidental future wiring without validation.
- Alternatively, apply `check_time_range` before any use, consistent with the other endpoints.

---

### FIND-12: Sensitive Request Bodies Logged at `debug` Level When `log_bodies = true`

**Severity:** Info
**Location:** `/crates/proxy/src/server/routes.rs` lines 720-728, 914-918, 1003-1007

**Description:**
When `LOG_BODIES=true` (env var) or `log_bodies` is set via the admin API, full request and response bodies are logged at `tracing::debug!`. Request bodies include the `messages` array, which may contain user PII, credentials, or sensitive business data. Response bodies may include tool call inputs/outputs.

The admin API warns about this at PUT time (routes.rs lines 574-579). However, when set via the `LOG_BODIES` env var, there is no startup warning.

Separately, `RUST_LOG=trace` exposes HTTP headers including `x-api-key` and `Authorization` values in the request ID middleware and auth middleware debug output. This is not guarded by `log_bodies`.

**Impact:** PII and credential exposure in logs if misconfigured. Operational risk, not a code vulnerability.

**Recommendation:**
- Add a `tracing::warn!` at startup when `LOG_BODIES=true` is detected from env (equivalent to the admin API warning).
- Document that `RUST_LOG=trace` exposes authentication headers.

---

### FIND-13: `is_safe_model_name` Allows Space Character

**Severity:** Info
**Location:** `/crates/proxy/src/admin/routes.rs` lines 152-159

**Description:**
The `is_safe_model_name` allowlist includes space in the set `"-_./: @"`. Model names with embedded spaces could appear in structured log output (e.g., `model_name = %body.model_name` in tracing events) and cause log injection if the logging backend does not properly escape field values. The risk is low because the logging is structured JSON (via `tracing_subscriber::fmt::layer().json()`), which escapes strings in JSON output.

**Impact:** Negligible. Structured JSON logging escapes the space correctly.

**Recommendation:**
- Remove space from the allowlist unless a specific provider requires it. No known provider naming convention uses spaces in model names. This reduces the attack surface for any future logging changes.

---

### FIND-14: Duplicate `axum` Versions in Dependency Tree

**Severity:** Info
**Location:** `Cargo.lock` lines 320 (axum 0.7.9) and 347 (axum 0.8.8)

**Description:**
The workspace includes both `axum 0.7.9` and `axum 0.8.8`. A transitive dependency (likely `axum-extra` or one of the AWS crates) requires 0.7.x. Dual major versions:
- Increase binary size (both are linked in).
- Create a potential future CVE exposure: if axum 0.7 receives a security advisory, it requires a separate fix path since cargo will not automatically upgrade a transitive dep across major versions.

**Impact:** No current security vulnerability. Increased binary size and potential future CVE maintenance burden.

**Recommendation:**
- Run `cargo tree -d` to identify which crate pulls in axum 0.7. Update or replace it if possible.

---

### FIND-15: Admin Token File Path Traversal via `ADMIN_TOKEN_PATH`

**Severity:** Info
**Location:** `/crates/proxy/src/main.rs` lines 767-773, 779-807

**Description:**
`ADMIN_TOKEN_PATH` is read from the environment and used as a file write path without sanitization:
```rust
Ok(p) => std::path::PathBuf::from(p),
```
An operator could inadvertently set `ADMIN_TOKEN_PATH=/etc/shadow` or another sensitive system path, causing the startup code to truncate and overwrite that file with the new token value (mode 0600 is applied after open, but `OpenOptions::truncate(true)` runs first on Unix).

The practical risk is low because:
- `ADMIN_TOKEN_PATH` is an operator-controlled env var.
- Writing a token value (a 36-character UUID or 64-character hex string) to `/etc/shadow` would corrupt it rather than expose credentials.
- The open fails if the path is not writable by the process user.

**Impact:** Operator foot-gun. Could overwrite system files if `ADMIN_TOKEN_PATH` is set to a path the operator did not intend.

**Recommendation:**
- Validate that `ADMIN_TOKEN_PATH` is an absolute path that does not traverse known sensitive locations (e.g., reject paths under `/etc/`).
- Alternatively, document the expected path format clearly and note that the value is treated as a file write path.

---

## Summary Table

| ID | Severity | Area | Status |
|---|---|---|---|
| FIND-01 | Critical* | Command injection via `execute_bash` | Mitigated by feature gate + PassThrough default; needs operator documentation |
| FIND-02 | High | `unsafe set_var` after Tokio runtime starts | Open (existing plan Task 8 adds assert but does not fix the race) |
| FIND-03 | High | Admin token is UUID v4 format | Open |
| FIND-04 | High | Langfuse SSRF gap | Open |
| FIND-05 | Medium | CSRF token cap race + endpoint not rate-limited | Open (partially addressed in existing plan Task 4) |
| FIND-06 | Medium | Free-text query params not length-capped | Open |
| FIND-07 | Medium | CSRF middleware missing PATCH method | Open |
| FIND-08 | Low | `read_file` allows unrestricted read when `allowed_dirs` empty | Open |
| FIND-09 | Low | `QDRANT_URL`/`REDIS_URL` not SSRF-validated | Open |
| FIND-10 | Low | Admin token not zeroized on drop | Open |
| FIND-11 | Low | Observability endpoint has unused timestamp fields | Open |
| FIND-12 | Info | `LOG_BODIES` lacks startup warning | Open |
| FIND-13 | Info | `is_safe_model_name` allows space | Open |
| FIND-14 | Info | Duplicate axum versions | Open |
| FIND-15 | Info | `ADMIN_TOKEN_PATH` not sanitized | Open |

*Critical severity applies only when `dangerous-builtin-tools` is compiled in and policy is `Allow`. In the default configuration, FIND-01 is not exploitable.

---

## Non-Findings (Verified Mitigations)

| Area | Mitigation |
|---|---|
| SQL injection | All queries use `rusqlite` parameterized binds. Dynamic SQL construction always uses `?` placeholders. |
| SSRF (backend URLs, OIDC, webhooks, MCP) | `validate_base_url()` at startup + `SsrfSafeDnsResolver` at connection time (feature-gated, enabled by default via `ssrf-protection` in client crate default features). |
| OIDC JWKS URI | JWKS URI from discovery document is validated via `validate_oidc_url` before fetch. |
| Auth timing attacks | API key comparisons use `subtle::ConstantTimeEq` on SHA-256/HMAC-SHA256 digests (fixed-size 32-byte). |
| CSRF | Double-submit cookie + one-time server-tracked token + Origin/Host header validation. Applied to POST, PUT, DELETE. |
| Path traversal (read_file) | `canonicalize()` before `allowed_dirs` prefix check. Absolute path required. Symlinks resolved. |
| Virtual key hashing | HMAC-SHA256 with per-installation CSPRNG secret (via `getrandom`). Dual-mode lookup supports legacy SHA-256 keys. |
| Admin server exposure | Bound to `127.0.0.1` only. `reject_cross_origin` middleware validates Origin/Host to localhost before any handler. DNS rebinding protected. |
| Request body size | 32 MB cap on proxy routes; 1 MB cap on admin routes. |
| Concurrency DoS | `Semaphore` with `try_acquire` (fail-fast 429, no queueing). |
| Header injection (request ID) | Invalid request IDs replaced with UUID rather than rejected or forwarded. |
| JWT validation | Audience + issuer validated. 60s clock skew tolerance. `jsonwebtoken` 10.3.0 (no known CVEs). |
| Unsafe Rust | `unsafe` used only for `set_var` at startup (see FIND-02). No `unsafe` in translator, client, or batch_engine crates. |
| Secret logging (env endpoint) | `redact_secret()` applied to all API keys and tokens in `GET /admin/api/env`. |
| IP allowlist spoofing | `TRUSTED_PROXY_DEPTH` rightmost-N logic. Left-side entries in `X-Forwarded-For` are attacker-controlled and ignored. |
| Log level injection | Admin API restricts `log_level` to an allowlist (`error/warn/info/debug`); arbitrary `RUST_LOG` directives are blocked. |
| Model name injection | `is_safe_model_name` blocks `..`, `?`, `#`, and non-alphanumeric characters beyond a safe set. |

---

## Recommended Fix Priority

1. **FIND-04** (Langfuse SSRF) -- one-line fix, high impact, no dependencies
2. **FIND-07** (CSRF missing PATCH) -- one-line fix, zero risk
3. **FIND-08** (read_file unrestricted) -- one-line behavior change in feature-gated code
4. **FIND-05** (CSRF cap race) -- medium effort, use moka; partially addressed in existing plan
5. **FIND-02** (unsafe set_var) -- architectural; the correct fix requires restructuring startup before `tokio::main`
6. **FIND-03** (admin token format) -- low effort, use getrandom directly
7. **FIND-06** (query param length caps) -- low effort, defensive
8. **FIND-09** (Qdrant/Redis SSRF) -- low effort, operator-controlled
9. **FIND-10** (zeroize token) -- low effort, `zeroize` already in Cargo.toml
