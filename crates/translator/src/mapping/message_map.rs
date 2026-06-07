//! Anthropic <-> OpenAI message/content mapping.
//!
//! Stateless conversion functions: `anthropic_to_openai_request` (forward translation),
//! `openai_to_anthropic_request` and `anthropic_to_openai_response` (reverse translation).
//! All functions are pure — no I/O, no side effects.

pub mod request;
pub mod response;
pub mod warnings;

#[cfg(test)]
mod tests;

pub use request::anthropic_to_openai_request;
pub use response::openai_to_anthropic_response;
pub use warnings::{compute_request_warnings, extract_system_text};
