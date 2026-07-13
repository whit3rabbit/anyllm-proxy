use std::sync::Arc;

/// Result of resolving a model name through the model router.
pub(crate) enum ResolvedModel {
    /// Routed via model_list to a specific backend and actual model name.
    Routed {
        backend_name: String,
        model: String,
        /// The deployment Arc for recording in-flight/latency stats.
        deployment: Arc<crate::config::model_router::Deployment>,
        /// Per-route option overrides when routed via a DB route; `None` when
        /// routed via the LiteLLM model_router (inherit global config).
        options: Option<Arc<crate::config::route_router::RouteOptions>>,
    },
    /// Model is known but all deployments are at their RPM limit.
    AllAtLimit,
    /// Model router is active but the model alias is not configured.
    UnknownModel,
    /// No model router, or model not in router. Used legacy ModelMapping.
    Legacy(String),
}
