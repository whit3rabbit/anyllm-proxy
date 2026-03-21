# Dependency Versions

## Translator Crate (`anthropic_openai_translate`)

| Dependency | Version | Features | Purpose |
|---|---|---|---|
| serde | 1.x | derive | Serialization/deserialization |
| serde_json | 1.x | - | JSON handling |
| thiserror | 2.x | - | Error derive macros |
| uuid | 1.x | v4 | ID generation |

### Dev Dependencies
| Dependency | Version | Purpose |
|---|---|---|
| pretty_assertions | 1.x | Readable test diffs |

## Proxy Crate (`anthropic_openai_proxy`)

| Dependency | Version | Features | Purpose |
|---|---|---|---|
| anthropic_openai_translate | path | - | Translation logic |
| axum | 0.8 | - | HTTP server framework |
| tokio | 1.x | full | Async runtime |
| reqwest | 0.12 | json, stream | HTTP client |
| serde | 1.x | derive | Serialization |
| serde_json | 1.x | - | JSON handling |
| futures | 0.3 | - | Stream combinators |
| tokio-stream | 0.1 | - | Stream wrappers |
| tower | 0.5 | limit | Middleware (concurrency limits) |
| tracing | 0.1 | - | Structured logging |
| tracing-subscriber | 0.3 | env-filter, json | Log formatting |
| uuid | 1.x | v4 | Request ID generation |

### Dev Dependencies
| Dependency | Version | Purpose |
|---|---|---|
| pretty_assertions | 1.x | Readable test diffs |
| reqwest | 0.12 | Integration test HTTP client |
| tokio | 1.x | Integration test runtime |

## Version Policy

- All dependencies use caret ranges (e.g., `"1"` means `>=1.0.0, <2.0.0`).
- No pinned versions; Cargo.lock (not committed) handles exact resolution.
- Minimum Rust edition: 2021.
