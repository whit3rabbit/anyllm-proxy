use crate::metrics::Metrics;
use std::collections::HashMap;
use std::sync::Arc;

/// Global state for the multi-backend metrics endpoint.
#[derive(Clone)]
pub(crate) struct GlobalState {
    pub(crate) backend_metrics: Arc<HashMap<String, Metrics>>,
}

/// Wrapper so OwnedSemaphorePermit can be stored in request extensions.
/// The field is never read directly; it exists as an RAII guard to hold
/// the permit until the struct is dropped.
#[derive(Clone)]
pub(crate) struct ConcurrencyPermit(
    #[allow(dead_code)] pub(crate) Arc<tokio::sync::OwnedSemaphorePermit>,
);
