pub mod env_aliases;
pub mod litellm;
pub mod model_router;
pub mod simple;
mod tls;
mod url_validation;

pub mod helpers;
pub mod multi;
pub mod single;
pub mod types;

pub use tls::TlsConfig;
pub use url_validation::{is_private_ip, validate_base_url, warn_if_cloud_metadata_url};

/// Process-global serial lock for tests that mutate or read environment
/// variables. Env vars are shared across the whole test binary, so tests in
/// different modules (env_aliases, litellm, single, ...) must serialize on the
/// SAME lock or they race (e.g. one test removing ANTHROPIC_BASE_URL while
/// another reads it). A per-module lock does not serialize across modules.
#[cfg(test)]
pub(crate) static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub use helpers::{
    extract_litellm_master_key, resolve_env_value, sanitize_api_key, strip_v1_suffix,
};
pub use multi::{BackendConfig, LoadResult, MultiConfig};
pub use single::Config;
pub use types::{BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat};
