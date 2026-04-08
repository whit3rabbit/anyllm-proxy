pub mod model;
pub mod provider;
pub mod providers;
pub mod registry;

pub use model::{ModelCapabilities, ModelDef, ModelStatus};
pub use provider::{AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus};
pub use registry::{
    all_providers, find_by_litellm_prefix, get_model, get_provider, list_models, resolve_backend,
};
