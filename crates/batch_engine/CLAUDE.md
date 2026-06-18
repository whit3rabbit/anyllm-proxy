# anyllm_batch_engine

Batch job orchestration: SQLite-backed queue, workers, file store, and event-driven webhook notifications. Powers the proxy's Batch API.

See root `../../CLAUDE.md` for workspace-wide commands and conventions.

## Test

```bash
cargo test -p anyllm_batch_engine
```

## Layout

- `engine.rs` — worker loop / orchestration.
- `job.rs` — job model + lifecycle states.
- `queue/sqlite.rs` — the durable queue (trait in `queue/mod.rs`).
- `db.rs` — schema + persistence.
- `file_store.rs` — input/output file blobs for batch jobs.
- `webhook/dispatcher.rs` + `webhook/sqlite.rs` — delivery with retry, durable state.
- `validation.rs` — request validation before enqueue.

## Gotchas

- Queue and webhook state are both persisted in SQLite — a restart resumes in-flight jobs. Don't assume in-memory state survives only the process; it does not for jobs.
- This crate owns batch persistence; the proxy's `batch/` module is the HTTP surface that calls into it. Keep the boundary clean.
