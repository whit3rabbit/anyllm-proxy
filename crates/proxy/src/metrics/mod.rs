// Request metrics: count, latency, error rates
// PLAN.md lines 867-870

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
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self) {
        self.inner.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_success(&self) {
        self.inner.requests_success.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.inner.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.inner.requests_total.load(Ordering::Relaxed),
            requests_success: self.inner.requests_success.load(Ordering::Relaxed),
            requests_error: self.inner.requests_error.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub requests_success: u64,
    pub requests_error: u64,
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
    fn metrics_clone_shares_state() {
        let m = Metrics::new();
        let m2 = m.clone();
        m.record_request();
        m2.record_request();
        assert_eq!(m.snapshot().requests_total, 2);
    }
}
