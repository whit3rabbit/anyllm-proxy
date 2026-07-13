//! IR types. Provider-agnostic; adapters in `anyllm_optimize_passes` build these
//! from OpenAI/Anthropic JSON and render them back.

/// Message role. Anthropic's system prompt is a top-level field, not a message; the
/// adapter synthesizes a `Role::System` message for it so protection is uniform.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// How a message may be treated. `Frozen` is informational (a message compressed on an
/// earlier turn that recomputes identically); the algorithm treats Frozen and Mutable
/// the same — both are eligible and produce the same bytes by the purity invariant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protection {
    /// Eligible for compression.
    Mutable,
    /// Compressed on an earlier turn (recomputed identically). Informational.
    Frozen,
    /// Never touch: system, latest user message, client-marked, unknown blocks.
    Immutable,
}

/// A whole conversation as received (client resends full history each turn).
#[derive(Clone, Debug, Default)]
pub struct Conversation {
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn new(messages: Vec<Message>) -> Self {
        Self { messages }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub protection: Protection,
    /// Client set `cache_control` on this message — never touch, never move.
    pub client_cache_marker: bool,
}

impl Message {
    /// Borrow the text of a compressible buffer by id, if it is a compressible kind.
    /// Returns `None` for immutable buffers (ToolUse/Opaque) or out-of-range ids.
    pub fn buffer(&self, id: BufferId) -> Option<&str> {
        match self.blocks.get(id.0) {
            Some(ContentBlock::Text(s)) => Some(s.as_str()),
            Some(ContentBlock::ToolResult { raw }) => Some(raw.as_str()),
            _ => None,
        }
    }
}

/// A content block. `ToolUse` args and `Opaque` (images, thinking, unknown) are never
/// edited. `ToolResult` gets value-level compression only (JSON structure preserved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentBlock {
    Text(String),
    /// JSON tool-call arguments — immutable (the model produced them; may be replayed).
    ToolUse {
        raw: String,
    },
    /// Tool output (JSON or text) — value-level compression only.
    ToolResult {
        raw: String,
    },
    /// Images, thinking, anything unrecognized — immutable passthrough.
    Opaque {
        raw: String,
    },
}

/// Index of a compressible buffer within a message's `blocks`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct BufferId(pub usize);

/// Identifies the decision procedure. Any change to model weights, rules, ratios, or
/// selection logic MUST bump this; operators expect one cache re-write when it changes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PolicyVersion(pub u64);
