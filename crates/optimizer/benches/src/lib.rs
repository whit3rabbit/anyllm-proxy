//! Shared corpus builders for the FFEC benchmarks. The benches live in `benches/`.

use anyllm_optimize_core::{ContentBlock, Conversation, Message, Protection, Role};

fn long_prose() -> String {
    "The quick brown fox jumps over the lazy dog again and again across the wide green \
     field toward the distant blue mountains far beyond the winding river and the trees."
        .repeat(3)
}

/// An `n`-turn user/assistant conversation of long prose messages.
pub fn prose_conversation(n: usize) -> Conversation {
    let text = long_prose();
    let messages = (0..n)
        .map(|i| Message {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            blocks: vec![ContentBlock::Text(text.clone())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        })
        .collect();
    Conversation::new(messages)
}

// --- ROADMAP §7 corpus classes -------------------------------------------------
//
// Each builder returns a `Conversation` shaped like one real traffic class so the
// bench exercises the code paths that matter for that class: RAG (one huge retrieved
// text buffer), tool-heavy (many ToolUse/ToolResult blocks), JSON (ToolResult JSON
// leaves), markdown (fenced + tables the segmenter must protect), and code (fences).
// The first user turn is Immutable (latest-message rule protects it), the rest Mutable.

fn user(blocks: Vec<ContentBlock>, protection: Protection) -> Message {
    Message {
        role: Role::User,
        blocks,
        protection,
        client_cache_marker: false,
    }
}

fn assistant(blocks: Vec<ContentBlock>) -> Message {
    Message {
        role: Role::Assistant,
        blocks,
        protection: Protection::Mutable,
        client_cache_marker: false,
    }
}

/// A ~`kb`-KB retrieved-context (RAG) conversation: several turns each carrying a large
/// retrieved-context buffer plus a short answer, so older turns are eligible history.
pub fn rag_conversation(kb: usize) -> Conversation {
    let para = long_prose();
    // Split the KB budget across 4 retrieval turns so multiple buffers are eligible.
    let per_turn = (kb * 1024 / 4 / para.len().max(1)).max(1);
    let retrieved = para.repeat(per_turn);
    let mut messages = Vec::with_capacity(8);
    for _ in 0..4 {
        messages.push(user(
            vec![ContentBlock::Text(format!(
                "Context:\n{retrieved}\n\nQuestion: what are the key points?"
            ))],
            Protection::Mutable,
        ));
        messages.push(assistant(vec![ContentBlock::Text(long_prose())]));
    }
    // Latest turn: the live question, protected.
    messages.push(user(
        vec![ContentBlock::Text(
            "Given all the context above, summarize the key points.".into(),
        )],
        Protection::Immutable,
    ));
    Conversation::new(messages)
}

/// A tool-heavy conversation: `n` rounds of (user ask, assistant ToolUse, tool ToolResult).
pub fn tool_conversation(n: usize) -> Conversation {
    let mut messages = Vec::with_capacity(n * 3);
    for i in 0..n {
        messages.push(assistant(vec![ContentBlock::Text(format!(
            "Let me look up record {i}. The following prose gives context. {}",
            long_prose()
        ))]));
        messages.push(assistant(vec![ContentBlock::ToolUse {
            raw: format!(r#"{{"name":"lookup","arguments":{{"id":{i}}}}}"#),
        }]));
        messages.push(Message {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult {
                raw: format!(
                    r#"{{"id":{i},"status":"ok","note":"{}","rows":[1,2,3]}}"#,
                    long_prose()
                ),
            }],
            protection: Protection::Mutable,
            client_cache_marker: false,
        });
    }
    // Protect the last message (latest-turn rule).
    if let Some(last) = messages.last_mut() {
        last.protection = Protection::Immutable;
    }
    Conversation::new(messages)
}

/// A JSON-heavy conversation: `n` tool results whose string leaves are compressible.
pub fn json_conversation(n: usize) -> Conversation {
    let note = long_prose();
    let messages = (0..n)
        .map(|i| Message {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult {
                raw: format!(
                    r#"{{"page":{i},"total":{n},"items":[{{"title":"{note}","body":"{note}"}}],"ok":true}}"#
                ),
            }],
            protection: if i + 1 == n {
                Protection::Immutable
            } else {
                Protection::Mutable
            },
            client_cache_marker: false,
        })
        .collect();
    Conversation::new(messages)
}

/// A markdown conversation: prose interleaved with tables and a fenced block per turn.
pub fn markdown_conversation(n: usize) -> Conversation {
    let body = format!(
        "# Heading\n\n{p}\n\n| col a | col b |\n|---|---|\n| {p} | {p} |\n\n```\nliteral fenced block {p}\n```\n\n{p}",
        p = long_prose()
    );
    let messages = (0..n)
        .map(|i| {
            let m = user(
                vec![ContentBlock::Text(body.clone())],
                if i == 0 {
                    Protection::Immutable
                } else {
                    Protection::Mutable
                },
            );
            Message {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                ..m
            }
        })
        .collect();
    Conversation::new(messages)
}

/// A code conversation: each turn is prose plus a fenced code block the segmenter protects.
pub fn code_conversation(n: usize) -> Conversation {
    let body = format!(
        "{p}\n\n```rust\nfn demo() {{\n    // {p}\n    let x = compute();\n    println!(\"{{x}}\");\n}}\n```\n\n{p}",
        p = long_prose()
    );
    let messages = (0..n)
        .map(|i| Message {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            blocks: vec![ContentBlock::Text(body.clone())],
            protection: if i == 0 {
                Protection::Immutable
            } else {
                Protection::Mutable
            },
            client_cache_marker: false,
        })
        .collect();
    Conversation::new(messages)
}
