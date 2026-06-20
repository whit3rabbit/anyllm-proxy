#[cfg(feature = "runtime-catalog")]
pub mod catalog;
pub mod model;
pub mod provider;
pub mod providers;
pub mod registry;

#[cfg(feature = "runtime-catalog")]
pub use catalog::{
    CatalogError, CatalogMetadata, OwnedModelDef, OwnedProviderDef, ProviderCatalog,
};
#[cfg(feature = "remote-catalog")]
pub use catalog::{RemoteCatalogOptions, DEFAULT_MAX_CATALOG_BYTES, LITELLM_CATALOG_URL};
pub use model::{ModelCapabilities, ModelDef, ModelStatus};
pub use provider::{AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus};
pub use registry::{
    all_providers, canonical_provider_id, find_by_litellm_prefix, get_model, get_provider,
    list_models, model_supports_anthropic_adaptive_thinking,
    model_supports_anthropic_reasoning_effort, resolve_backend,
};
