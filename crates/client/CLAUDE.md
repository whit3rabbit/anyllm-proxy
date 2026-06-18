# anyllm_client

Async HTTP client for talking to Anthropic-style backends: retry policy, SSRF protection, SSE streaming, rate limiting.

See root `../../CLAUDE.md` for workspace-wide commands and conventions.

## Test

```bash
cargo test -p anyllm_client
```

## Features

- `default = ["ssrf-protection", "native-tls"]`.
- **At least one TLS backend (`native-tls` or `rustls`) must be enabled** or the crate fails to compile with an actionable error (see `lib.rs`).
- `ssrf-protection` gates the URL/IP filtering in `http.rs`.

## Layout

- `retry.rs` — `send_with_retry_policy` is the **canonical** retry loop for the whole workspace. The proxy's `backend/mod.rs::send_with_retry` delegates here.
- `http.rs` — `build_http_client`, SSRF filtering, header assembly.
- `sse.rs` + `streaming.rs` — `run_sse_task` is the shared SSE frame reader.
- `anthropic_client.rs` — delegates to `retry::send_with_retry_policy` (no hand-rolled loop).
- `rate_limit.rs`, `tools.rs`, `error.rs`.

## Gotchas

- **`reqwest::Error::is_timeout()` fires on read timeouts too.** A read timeout means the POST was already processed. Only `is_connect()` is safe to retry on POST. `retry_transport_errors` gates on `is_connect()` only by design — do NOT add `is_timeout()`.
- **Two retry loops exist** (this crate's canonical `retry.rs` and proxy's `bedrock_client.rs`). A backoff/classification change must hit both or they diverge silently. `anthropic_client.rs` delegates to `retry.rs`, so it tracks automatically.
- **`2u64.pow(attempt)` overflows at attempt >= 64.** `backoff_delay` caps with `attempt.min(62)`. Any new backoff formula needs the same guard.
- **Backoff jitter is deterministic** (upper bound, not random) to keep tests predictable.
- **`Ipv4Addr::is_broadcast()` matches only `255.255.255.255`**, not directed broadcasts like `10.0.0.255`. Those slip through SSRF filters when `allow_private=true`.
- **`extra_headers` Vec + `HeaderMap::insert` = last writer wins.** Push builder-level headers to the END of the Vec so they overwrite caller duplicates. Index 0 loses.
- **Header `&str` slices:** when building `&[(&str, &str)]` from a `HeaderMap`, collect owned `String` locals first; the borrow checker rejects inline `.to_str()`.
- **Avoid `.clone()` before serialization when only one field changes.** Use `serde_json::to_value(req)` + patch — `MessageCreateRequest` clone is O(content size).
