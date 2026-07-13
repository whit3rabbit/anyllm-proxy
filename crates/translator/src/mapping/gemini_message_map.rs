// Anthropic Messages API <-> Gemini generateContent API message mapping.
//
// Pure translation functions, no IO. Split by direction:
//   - `request`  Anthropic -> Gemini request
//   - `response` Gemini -> Anthropic response
//   - `reverse`  Gemini -> Anthropic request & Anthropic -> Gemini response
//   - `helpers`  shared utilities (role merging, tool-id mapping)

mod helpers;
mod request;
mod response;
mod reverse;

pub use helpers::{build_tool_id_map, merge_consecutive_roles};
pub use request::{anthropic_to_gemini_request, compute_gemini_request_warnings};
pub use response::gemini_to_anthropic_response;
pub use reverse::{anthropic_to_gemini_response, gemini_to_anthropic_request};

#[cfg(test)]
mod tests;
