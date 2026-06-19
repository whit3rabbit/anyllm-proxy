use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::insert_request_log;
use crate::admin::state::RequestLogEntry;

/// Spawn the write buffer background task. Returns the sender for proxy handlers.
/// Flushes every 100ms or 100 rows, whichever comes first.
pub fn spawn_write_buffer(db: Arc<Mutex<Connection>>) -> mpsc::Sender<RequestLogEntry> {
    let (tx, mut rx) = mpsc::channel::<RequestLogEntry>(1024);

    tokio::spawn(async move {
        let mut buf: Vec<RequestLogEntry> = Vec::with_capacity(128);
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                maybe_entry = rx.recv() => {
                    match maybe_entry {
                        Some(entry) => {
                            buf.push(entry);
                            if buf.len() >= 100 {
                                flush_buffer(&db, &mut buf).await;
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit.
                            if !buf.is_empty() {
                                flush_buffer(&db, &mut buf).await;
                            }
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buf.is_empty() {
                        flush_buffer(&db, &mut buf).await;
                    }
                }
            }
        }
    });

    tx
}

async fn flush_buffer(db: &Arc<Mutex<Connection>>, buf: &mut Vec<RequestLogEntry>) {
    let entries = std::mem::take(buf);
    let db = db.clone();
    // Run SQLite IO on the blocking threadpool to avoid stalling the tokio executor.
    // On failure, return the entries so they can be re-queued for retry.
    let result = tokio::task::spawn_blocking(move || {
        // Mutex poisoning recovery: if a prior request panicked while holding the lock,
        // we recover the inner value rather than permanently locking the database.
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = (|| -> rusqlite::Result<()> {
            let tx = conn.unchecked_transaction()?;
            for entry in &entries {
                insert_request_log(&tx, entry)?;
            }
            tx.commit()?;
            Ok(())
        })() {
            tracing::error!(error = %e, count = entries.len(), "failed to flush request log buffer");
            Some(entries)
        } else {
            None
        }
    })
    .await;

    // On failure, re-queue entries so they can be retried on the next flush.
    if let Ok(Some(mut entries)) = result {
        buf.append(&mut entries);
        // Cap retry buffer to prevent unbounded growth on persistent DB failure.
        const MAX_RETRY_BUFFER: usize = 1000;
        if buf.len() > MAX_RETRY_BUFFER {
            let dropped = buf.len() - MAX_RETRY_BUFFER;
            buf.drain(..dropped);
            tracing::warn!(dropped, "dropped oldest log entries to cap retry buffer");
        }
    }
}
