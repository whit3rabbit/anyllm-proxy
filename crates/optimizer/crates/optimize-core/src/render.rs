//! Renderer: apply validated edit scripts to the IR, producing a new (compressed)
//! conversation. Provider-agnostic; adapters in `anyllm_optimize_passes` turn a
//! `RenderedConversation` back into OpenAI/Anthropic JSON.

use crate::edit::EditScript;
use crate::types::{BufferId, ContentBlock, Conversation, Message};

#[derive(Clone, Debug)]
pub struct RenderedMessage {
    pub blocks: Vec<ContentBlock>,
}

#[derive(Clone, Debug)]
pub struct RenderedConversation {
    pub messages: Vec<RenderedMessage>,
    /// Message index at which the deepest cache breakpoint should sit (the frontier),
    /// for providers with explicit breakpoints. `None` = provider manages caching.
    pub breakpoint: Option<usize>,
}

/// Apply `edits` (each `(msg_idx, buffer_id, script)`) to a clone of `conv`. Scripts
/// must already be validated. Buffers not targeted are copied verbatim.
pub fn render(
    conv: &Conversation,
    edits: &[(usize, BufferId, EditScript)],
    breakpoint: Option<usize>,
) -> RenderedConversation {
    let mut messages: Vec<RenderedMessage> = conv
        .messages
        .iter()
        .map(|m: &Message| RenderedMessage {
            blocks: m.blocks.clone(),
        })
        .collect();

    let mut buf = String::new();
    for (mi, bid, script) in edits {
        let Some(msg) = messages.get_mut(*mi) else {
            continue;
        };
        let Some(block) = msg.blocks.get_mut(bid.0) else {
            continue;
        };
        match block {
            ContentBlock::Text(s) => {
                script.apply(s, &mut buf);
                std::mem::swap(s, &mut buf);
            }
            ContentBlock::ToolResult { raw } => {
                script.apply(raw, &mut buf);
                std::mem::swap(raw, &mut buf);
            }
            // ToolUse / Opaque are immutable — never targeted; ignore defensively.
            _ => {}
        }
    }

    RenderedConversation {
        messages,
        breakpoint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;
    use crate::types::{Protection, Role};

    fn msg(text: &str) -> Message {
        Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.into())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        }
    }

    #[test]
    fn applies_edit_to_targeted_buffer() {
        let conv = Conversation::new(vec![msg("hello world"), msg("keep me")]);
        let edits = vec![(
            0usize,
            BufferId(0),
            EditScript::new(vec![Edit::Delete(5..11)]), // drop " world"
        )];
        let out = render(&conv, &edits, Some(1));
        assert_eq!(
            out.messages[0].blocks[0],
            ContentBlock::Text("hello".into())
        );
        assert_eq!(
            out.messages[1].blocks[0],
            ContentBlock::Text("keep me".into())
        );
        assert_eq!(out.breakpoint, Some(1));
    }
}
