// Anthropic passthrough handlers module: forwards raw request bytes to the real Anthropic API.
// No translation: the proxy receives Anthropic format and returns Anthropic format.

pub(crate) mod auth;
pub(crate) mod handlers;

pub(crate) use handlers::{anthropic_generic_passthrough, anthropic_passthrough};
