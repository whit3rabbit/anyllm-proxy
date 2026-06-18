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

    #[cfg(feature = "otel")]
    let guard = {
        let (g, tracer) = anyllm_proxy::otel::init_otel();
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .with(otel_layer)
            .init();
        g
    };

    #[cfg(not(feature = "otel"))]
    #[allow(clippy::let_unit_value)]
    let guard = {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    };

    (guard, reload_handle)
}
