//! Configuration loading, parsing, and management for the proxy server.
//!
//! Supports single-backend (legacy) configurations, LiteLLM config syntax,
//! environment variables mapping, and multi-backend routing setups.

/// Environment variable aliases for LiteLLM/OpenAI/Anthropic keys.
pub mod env_aliases;
/// LiteLLM-compatible YAML configuration format parser.
pub mod litellm;
/// Dynamic routing rules mapping request model names to backends.
pub mod model_router;
/// DB-backed API route dispatcher.
pub mod route_router;
/// Simple YAML/TOML configuration parser.
pub mod simple;
mod tls;
mod url_validation;

/// Helper functions for resolving environment configurations.
pub mod helpers;
/// Configuration representation for multi-backend routing.
pub mod multi;
/// Configuration representation for a single static backend.
pub mod single;
/// Common backend configuration type definitions.
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
    env_bool_flag, extract_litellm_master_key, resolve_env_value, sanitize_api_key, strip_v1_suffix,
};
pub use multi::{BackendConfig, LoadResult, MultiConfig};
pub use single::Config;
pub use types::{BackendAuth, BackendKind, ModelMapping, OpenAIApiFormat};
