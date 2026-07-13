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
    pxpipe_compressed_total: AtomicU64,
    pxpipe_images_total: AtomicU64,
    pxpipe_imaged_chars_total: AtomicU64,
    rtk_compressed_total: AtomicU64,
    rtk_blocks_total: AtomicU64,
    rtk_saved_chars_total: AtomicU64,
    optimizer_compressed_total: AtomicU64,
    optimizer_messages_compressed_total: AtomicU64,
    optimizer_removed_tokens_total: AtomicU64,
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

    /// Record one pxpipe text-to-image compression: `images` PNG blocks emitted
    /// standing in for `imaged_chars` source chars. Called on the compressed path
    /// only (both Anthropic passthrough and translate).
    pub fn record_pxpipe_compression(&self, images: u64, imaged_chars: u64) {
        self.inner
            .pxpipe_compressed_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .pxpipe_images_total
            .fetch_add(images, Ordering::Relaxed);
        self.inner
            .pxpipe_imaged_chars_total
            .fetch_add(imaged_chars, Ordering::Relaxed);
    }

    /// Record one RTK tool-output compression: `blocks` tool-result payloads
    /// rewritten, saving `saved_chars` source chars. Called on the compressed
    /// path only (Anthropic passthrough + translate).
    pub fn record_rtk_compression(&self, blocks: u64, saved_chars: u64) {
        self.inner
            .rtk_compressed_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .rtk_blocks_total
            .fetch_add(blocks, Ordering::Relaxed);
        self.inner
            .rtk_saved_chars_total
            .fetch_add(saved_chars, Ordering::Relaxed);
    }

    /// Record one FFEC prompt-optimizer compression: `messages_compressed`
    /// history messages rewritten, saving an estimated `removed_tokens_est`
    /// tokens. Called on the applied (Live, `report.applied == true`) path only
    /// -- Shadow mode never mutates the request and never calls this.
    pub fn record_optimization(&self, messages_compressed: u64, removed_tokens_est: u64) {
        self.inner
            .optimizer_compressed_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .optimizer_messages_compressed_total
            .fetch_add(messages_compressed, Ordering::Relaxed);
        self.inner
            .optimizer_removed_tokens_total
            .fetch_add(removed_tokens_est, Ordering::Relaxed);
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
            pxpipe_compressed_total: self.inner.pxpipe_compressed_total.load(Ordering::Relaxed),
            pxpipe_images_total: self.inner.pxpipe_images_total.load(Ordering::Relaxed),
            pxpipe_imaged_chars_total: self.inner.pxpipe_imaged_chars_total.load(Ordering::Relaxed),
            rtk_compressed_total: self.inner.rtk_compressed_total.load(Ordering::Relaxed),
            rtk_blocks_total: self.inner.rtk_blocks_total.load(Ordering::Relaxed),
            rtk_saved_chars_total: self.inner.rtk_saved_chars_total.load(Ordering::Relaxed),
            optimizer_compressed_total: self
                .inner
                .optimizer_compressed_total
                .load(Ordering::Relaxed),
            optimizer_messages_compressed_total: self
                .inner
                .optimizer_messages_compressed_total
                .load(Ordering::Relaxed),
            optimizer_removed_tokens_total: self
                .inner
                .optimizer_removed_tokens_total
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
    /// Requests where pxpipe text-to-image compression fired.
    pub pxpipe_compressed_total: u64,
    /// Total PNG image blocks pxpipe emitted across all compressed requests.
    pub pxpipe_images_total: u64,
    /// Total source chars pxpipe replaced with images.
    pub pxpipe_imaged_chars_total: u64,
    /// Requests where RTK tool-output compression fired.
    pub rtk_compressed_total: u64,
    /// Total tool-result payloads RTK rewrote across all compressed requests.
    pub rtk_blocks_total: u64,
    /// Total source chars RTK removed from tool output.
    pub rtk_saved_chars_total: u64,
    /// Requests where the FFEC prompt optimizer applied a compression (Live mode only).
    pub optimizer_compressed_total: u64,
    /// Total history messages the optimizer rewrote across all applied requests.
    pub optimizer_messages_compressed_total: u64,
    /// Total estimated tokens the optimizer removed across all applied requests.
    pub optimizer_removed_tokens_total: u64,
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
    fn pxpipe_metrics_counting() {
        let m = Metrics::new();
        m.record_pxpipe_compression(3, 12_000);
        m.record_pxpipe_compression(2, 8_000);
        let s = m.snapshot();
        assert_eq!(s.pxpipe_compressed_total, 2);
        assert_eq!(s.pxpipe_images_total, 5);
        assert_eq!(s.pxpipe_imaged_chars_total, 20_000);
    }

    #[test]
    fn optimizer_metrics_counting() {
        let m = Metrics::new();
        m.record_optimization(3, 1_200);
        m.record_optimization(2, 800);
        let s = m.snapshot();
        assert_eq!(s.optimizer_compressed_total, 2);
        assert_eq!(s.optimizer_messages_compressed_total, 5);
        assert_eq!(s.optimizer_removed_tokens_total, 2_000);
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
