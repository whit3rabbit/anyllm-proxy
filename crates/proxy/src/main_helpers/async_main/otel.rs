use std::io::IsTerminal;
use tracing_subscriber::prelude::*;

#[cfg(feature = "otel")]
pub type OtelGuard = anyllm_proxy::otel::OtelGuard;

#[cfg(not(feature = "otel"))]
pub type OtelGuard = ();

pub fn init_tracing() -> (
    OtelGuard,
    tracing_subscriber::reload::Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>,
) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);

    // Human-readable when stdout is a terminal; JSON when piped (Docker/systemd).
    // LOG_FORMAT=json|text overrides the auto-detection.
    let use_json = match std::env::var("LOG_FORMAT").ok().as_deref() {
        Some("json") => true,
        Some("text") | Some("pretty") | Some("human") => false,
        _ => !std::io::stdout().is_terminal(),
    };
    let fmt_layer = if use_json {
        tracing_subscriber::fmt::layer().json().boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    #[cfg(feature = "otel")]
    let guard = {
        let (g, tracer) = anyllm_proxy::otel::init_otel();
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
        g
    };

    #[cfg(not(feature = "otel"))]
    #[allow(clippy::let_unit_value)]
    let guard = {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
    };

    (guard, reload_handle)
}
