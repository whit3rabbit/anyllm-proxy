//! Provider adapters: `serde_json::Value` (a parsed request body) ⇄ IR.
//!
//! Two roles per adapter:
//! - `from_value(&Value) -> Conversation`: build the IR the optimizer reasons over.
//! - `apply_rendered(&mut Value, &RenderedConversation)`: write compressed text back
//!   INTO the original body by index, preserving every field the optimizer did not
//!   touch (tool_calls, names, ids, metadata). We never reconstruct the body from IR —
//!   that would drop unknown fields (mirrors the proxy's in-place mutation approach and
//!   the `cache_control`-drop gotcha in the proxy `CLAUDE.md`).

pub mod anthropic;
pub mod openai;

use anyllm_optimize_core::{ContentBlock, Protection, Role};

/// Shared: map a wire role string to the IR role.
pub(crate) fn parse_role(s: Option<&str>) -> Role {
    match s {
        Some("system") | Some("developer") => Role::System,
        Some("assistant") | Some("model") => Role::Assistant,
        Some("tool") | Some("function") => Role::Tool,
        _ => Role::User,
    }
}

/// Shared: protection for message at index `i` of `n` given its role and whether the
/// client marked it with a cache breakpoint. The latest message and system are Immutable.
pub(crate) fn protection_for(role: Role, i: usize, n: usize, client_marked: bool) -> Protection {
    if role == Role::System || client_marked || i + 1 >= n {
        Protection::Immutable
    } else {
        Protection::Mutable
    }
}

/// Shared: wrap a role's plain-string content as the right block kind.
pub(crate) fn string_block(role: Role, s: &str) -> ContentBlock {
    if role == Role::Tool {
        ContentBlock::ToolResult { raw: s.to_string() }
    } else {
        ContentBlock::Text(s.to_string())
    }
}
