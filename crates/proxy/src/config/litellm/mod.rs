//! LiteLLM config.yaml parser.
//!
//! Accepts LiteLLM's YAML config format (model_list, litellm_settings,
//! router_settings, general_settings) and converts it to anyllm-proxy's
//! MultiConfig + ModelRouter.
//!
//! [`types`] holds the Serde structs mapping the LiteLLM schema; [`parser`]
//! holds the conversion functions.

mod parser;
mod types;

pub(crate) use parser::parse_routing_strategy_str;
pub use parser::{extract_master_key, from_litellm_yaml, parse_litellm_yaml};
pub use types::LiteLLMParsed;

#[cfg(test)]
mod tests;
