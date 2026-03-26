//! Optional OpenTelemetry OTLP trace export.
//!
//! Enabled by the `otel` cargo feature. All types and functions in this module
//! are gated behind `#[cfg(feature = "otel")]` at the module declaration site
//! in `lib.rs`, so nothing here compiles into the default binary.
//!
//! The OTLP SDK reads configuration from standard env vars:
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` (default `http://localhost:4318` for HTTP)
//! - `OTEL_SERVICE_NAME`
//! - `OTEL_TRACES_SAMPLER` / `OTEL_TRACES_SAMPLER_ARG`

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Holds the [`SdkTracerProvider`] and flushes pending spans on drop.
pub struct OtelGuard {
    provider: SdkTracerProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            eprintln!("otel: tracer provider shutdown error: {e}");
        }
    }
}

/// Initialise the OTLP span exporter and return a guard that must live for the
/// duration of `main`. The returned tracer is suitable for
/// [`tracing_opentelemetry::OpenTelemetryLayer::new`].
///
/// Panics if the exporter or provider cannot be created (misconfiguration).
pub fn init_otel() -> (OtelGuard, opentelemetry_sdk::trace::Tracer) {
    // HTTP/protobuf transport via reqwest (matches Cargo feature flags).
    let exporter = SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to create OTLP span exporter");

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer("anyllm-proxy");
    let guard = OtelGuard { provider };

    (guard, tracer)
}
