// Request metrics: count, latency, error rates

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Simple in-memory metrics counters.
/// For production, replace with prometheus or similar.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug, Default)]
struct MetricsInner {
    requests_total: AtomicU64,
    requests_success: AtomicU64,
    requests_error: AtomicU64,
    streams_started: AtomicU64,
    streams_completed: AtomicU64,
    streams_failed: AtomicU64,
    streams_client_disconnected: AtomicU64,
}

impl Metrics {
    /// Create a new zero-valued metrics counter.
    pub fn new() -> Self {
        Self::default()
    }

    // Relaxed ordering: these are independent counters with no cross-counter
    // invariants, so no synchronization is needed. Relaxed is fastest.

    /// Increment the total request counter. Called once per proxied request.
    pub fn record_request(&self) {
        self.inner.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the success counter (backend returned 2xx).
    pub fn record_success(&self) {
        self.inner.requests_success.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the error counter (backend returned non-2xx or transport failure).
    pub fn record_error(&self) {
        self.inner.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment when an SSE stream begins sending events to the client.
    pub fn record_stream_started(&self) {
        self.inner.streams_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment when an SSE stream completes normally (backend sent all data).
    pub fn record_stream_completed(&self) {
        self.inner.streams_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment when an SSE stream fails due to an upstream error.
    pub fn record_stream_failed(&self) {
        self.inner.streams_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment when the downstream client disconnects before the stream finishes.
    pub fn record_stream_client_disconnected(&self) {
        self.inner
            .streams_client_disconnected
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Take a point-in-time snapshot of all counters for the GET /metrics endpoint.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.inner.requests_total.load(Ordering::Relaxed),
            requests_success: self.inner.requests_success.load(Ordering::Relaxed),
            requests_error: self.inner.requests_error.load(Ordering::Relaxed),
            streams_started: self.inner.streams_started.load(Ordering::Relaxed),
            streams_completed: self.inner.streams_completed.load(Ordering::Relaxed),
            streams_failed: self.inner.streams_failed.load(Ordering::Relaxed),
            streams_client_disconnected: self
                .inner
                .streams_client_disconnected
                .load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time snapshot of counters, serialized as JSON for GET /metrics.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MetricsSnapshot {
    /// Total proxied requests (success + error + in-flight).
    pub requests_total: u64,
    /// Requests where the backend returned a 2xx status.
    pub requests_success: u64,
    /// Requests that failed (non-2xx status or transport error).
    pub requests_error: u64,
    /// SSE streams that began sending events to the client.
    pub streams_started: u64,
    /// SSE streams that completed normally.
    pub streams_completed: u64,
    /// SSE streams that failed due to upstream errors.
    pub streams_failed: u64,
    /// SSE streams where the client disconnected early.
    pub streams_client_disconnected: u64,
}

impl MetricsSnapshot {
    /// Fraction of requests that resulted in errors (0.0 when no requests).
    pub fn error_rate(&self) -> f64 {
        if self.requests_total > 0 {
            self.requests_error as f64 / self.requests_total as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_counting() {
        let m = Metrics::new();
        m.record_request();
        m.record_request();
        m.record_success();
        m.record_error();

        let s = m.snapshot();
        assert_eq!(s.requests_total, 2);
        assert_eq!(s.requests_success, 1);
        assert_eq!(s.requests_error, 1);
    }

    #[test]
    fn streaming_metrics_counting() {
        let m = Metrics::new();
        m.record_stream_started();
        m.record_stream_started();
        m.record_stream_started();
        m.record_stream_completed();
        m.record_stream_failed();
        m.record_stream_client_disconnected();

        let s = m.snapshot();
        assert_eq!(s.streams_started, 3);
        assert_eq!(s.streams_completed, 1);
        assert_eq!(s.streams_failed, 1);
        assert_eq!(s.streams_client_disconnected, 1);
    }

    #[test]
    fn metrics_clone_shares_state() {
        let m = Metrics::new();
        let m2 = m.clone();
        m.record_request();
        m2.record_request();
        assert_eq!(m.snapshot().requests_total, 2);
    }
}
