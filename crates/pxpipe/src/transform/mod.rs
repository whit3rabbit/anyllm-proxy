//! Request-body transforms. IO-free: pure functions over `serde_json::Value`.

mod anthropic;
mod factsheet;
mod gate;
mod info;
mod openai;
mod schema_strip;

pub use anthropic::{transform as transform_anthropic, AnthropicOpts};
pub use info::TransformInfo;
pub use openai::{transform as transform_openai_chat, GptOpts};
