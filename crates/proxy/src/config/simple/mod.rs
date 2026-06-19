//! Simple native YAML config format for anyllm-proxy.
//!
//! Activated when the config file contains a top-level `models:` key
//! (as opposed to LiteLLM's `model_list:`).

pub mod parser;
pub mod tool_builder;
pub mod types;

#[cfg(test)]
mod tests;

pub use parser::parse_simple_yaml;
pub use types::{
    BuiltinToolConfig, McpServerConfig, SimpleConfig, SimpleModelEntry, SimpleModelFull,
    SimpleParsed, ToolExecutionConfig, ToolStartupConfig,
};
