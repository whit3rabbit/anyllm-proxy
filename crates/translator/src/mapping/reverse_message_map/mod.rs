// Reverse message mapping: OpenAI Chat Completions -> Anthropic Messages
//
// Converts OpenAI-format requests to Anthropic format (for accepting OpenAI
// input) and Anthropic responses back to OpenAI format.
//
// - [`context`]  request-local sanitized <-> original tool-name mapping
// - [`request`]  OpenAI request -> Anthropic request
// - [`response`] Anthropic response -> OpenAI response

mod context;
mod request;
mod response;

pub use context::AnthropicTranslationContext;
pub use request::{
    compute_openai_request_warnings, openai_to_anthropic_request,
    openai_to_anthropic_request_with_context,
};
pub use response::{
    anthropic_stop_reason_to_openai, anthropic_to_openai_response,
    anthropic_to_openai_response_with_context,
};

#[cfg(test)]
mod tests;
