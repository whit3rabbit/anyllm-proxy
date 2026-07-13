use super::*;
use crate::thinking_repair::store::ThinkingRepairStore;
use anyllm_translate::anthropic::{Content, InputMessage, MessageCreateRequest};
use serde_json::json;

fn thinking(text: &str, sig: &str) -> ContentBlock {
    ContentBlock::Thinking {
        thinking: text.to_string(),
        signature: Some(sig.to_string()),
    }
}

fn tool_use(id: &str) -> ContentBlock {
    ContentBlock::ToolUse {
        id: id.to_string(),
        name: "get_weather".to_string(),
        input: json!({"city": "nyc"}),
    }
}

fn text(s: &str) -> ContentBlock {
    ContentBlock::Text {
        text: s.to_string(),
    }
}

fn req_with_last_assistant(blocks: Vec<ContentBlock>) -> MessageCreateRequest {
    MessageCreateRequest {
        model: "claude-opus-4-5".to_string(),
        max_tokens: 1024,
        messages: vec![
            InputMessage {
                role: Role::User,
                content: Content::Text("hi".to_string()),
            },
            InputMessage {
                role: Role::Assistant,
                content: Content::Blocks(blocks),
            },
        ],
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: None,
        extra: serde_json::Map::new(),
    }
}

fn last_blocks(req: &MessageCreateRequest) -> &Vec<ContentBlock> {
    match &req.messages.last().unwrap().content {
        Content::Blocks(b) => b,
        Content::Text(_) => panic!("expected blocks"),
    }
}

#[tokio::test]
async fn tier0_byte_identical_passthrough() {
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![thinking("hmm", "sig_1"), tool_use("toolu_1")],
        )
        .await;

    let mut req = req_with_last_assistant(vec![thinking("hmm", "sig_1"), tool_use("toolu_1")]);
    let result = repair_request(&store, "ns1", &mut req).await;

    assert!(
        result.is_none(),
        "byte-identical replay should not be touched"
    );
    assert_eq!(last_blocks(&req).len(), 2);
}

#[tokio::test]
async fn tier1_restores_mutated_text_under_known_signature() {
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![thinking("original thought", "sig_1"), tool_use("toolu_1")],
        )
        .await;

    let mut req = req_with_last_assistant(vec![
        thinking("merged garbled thought", "sig_1"),
        tool_use("toolu_1"),
    ]);
    let result = repair_request(&store, "ns1", &mut req).await;

    assert!(result.unwrap().contains("restored 1"));
    match &last_blocks(&req)[0] {
        ContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, "original thought"),
        other => panic!("expected restored Thinking block, got {other:?}"),
    }
}

#[tokio::test]
async fn tier2_drops_intruder_block_from_different_message() {
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![thinking("owner thought", "sig_owner"), tool_use("toolu_1")],
        )
        .await;
    store
        .commit("ns1", "msg_2", vec![thinking("other thought", "sig_other")])
        .await;

    // Replay claims to be msg_1's turn (tool_use toolu_1) but carries an
    // intruder thinking block whose signature belongs to msg_2.
    let mut req = req_with_last_assistant(vec![
        thinking("other thought", "sig_other"),
        thinking("owner thought", "sig_owner"),
        tool_use("toolu_1"),
    ]);
    let result = repair_request(&store, "ns1", &mut req).await;

    assert!(result.unwrap().contains("dropped 1"));
    let blocks = last_blocks(&req);
    assert_eq!(blocks.len(), 2);
    assert!(
        matches!(&blocks[0], ContentBlock::Thinking { signature: Some(s), .. } if s == "sig_owner")
    );
}

#[tokio::test]
async fn unknown_signature_with_known_owner_is_dropped() {
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![thinking("owner thought", "sig_owner"), tool_use("toolu_1")],
        )
        .await;

    let mut req = req_with_last_assistant(vec![
        thinking("garbage, no matching record", "sig_unknown"),
        thinking("owner thought", "sig_owner"),
        tool_use("toolu_1"),
    ]);
    let result = repair_request(&store, "ns1", &mut req).await;

    assert!(result.unwrap().contains("dropped 1"));
    assert_eq!(last_blocks(&req).len(), 2);
}

#[tokio::test]
async fn unknown_signature_with_no_owner_fails_open() {
    let store = ThinkingRepairStore::new();
    // No tool_use in this turn, so no owner is resolvable.
    let mut req = req_with_last_assistant(vec![
        thinking("standalone thought", "sig_unknown"),
        text("done"),
    ]);
    let result = repair_request(&store, "ns1", &mut req).await;

    assert!(
        result.is_none(),
        "no owner evidence -> fail open, pass through"
    );
    assert_eq!(last_blocks(&req).len(), 2);
}

#[tokio::test]
async fn reinserts_lost_redacted_thinking_block() {
    let store = ThinkingRepairStore::new();
    let redacted = ContentBlock::RedactedThinking {
        data: "encrypted-blob".to_string(),
    };
    store
        .commit("ns1", "msg_1", vec![redacted.clone(), tool_use("toolu_1")])
        .await;

    // Client replay is missing the redacted_thinking block entirely
    // (Claude Code doesn't persist it to JSONL).
    let mut req = req_with_last_assistant(vec![tool_use("toolu_1")]);
    let result = repair_request(&store, "ns1", &mut req).await;

    let msg = result.unwrap();
    assert!(msg.contains("reinserted"), "got: {msg}");
    let blocks = last_blocks(&req);
    assert_eq!(blocks.len(), 2);
    assert!(
        matches!(&blocks[0], ContentBlock::RedactedThinking { data } if data == "encrypted-blob")
    );
}

#[tokio::test]
async fn surplus_current_nonthinking_block_disables_reinsert() {
    // rec has [thinking, tool_use_1]; current turn replays tool_use_1
    // AND adds an extra tool_use_2 that was never recorded, while also
    // dropping the thinking block. Non-thinking counts don't line up
    // (2 vs. 1), so positional reinsertion would either drop toolu_2 or
    // misalign it against rec -- must fail open instead.
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![thinking("hmm", "sig_1"), tool_use("toolu_1")],
        )
        .await;

    let mut req = req_with_last_assistant(vec![tool_use("toolu_1"), tool_use("toolu_2")]);
    let result = repair_request(&store, "ns1", &mut req).await;
    let blocks = last_blocks(&req);

    assert!(result.is_none(), "count mismatch should fail open");
    assert_eq!(
        blocks.len(),
        2,
        "current turn's blocks must be preserved as-is"
    );
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { id, .. } if id == "toolu_2")),
        "toolu_2 must not be dropped"
    );
}

#[tokio::test]
async fn missing_current_nonthinking_block_disables_reinsert() {
    // rec has [thinking, tool_use_1, tool_use_2]; current turn only
    // replays tool_use_1 (dropped tool_use_2 AND the thinking block).
    // Non-thinking counts don't line up (1 vs. 2), so reinserting
    // positionally would resurrect stale toolu_2 -- must fail open.
    let store = ThinkingRepairStore::new();
    store
        .commit(
            "ns1",
            "msg_1",
            vec![
                thinking("hmm", "sig_1"),
                tool_use("toolu_1"),
                tool_use("toolu_2"),
            ],
        )
        .await;

    let mut req = req_with_last_assistant(vec![tool_use("toolu_1")]);
    let result = repair_request(&store, "ns1", &mut req).await;
    let blocks = last_blocks(&req);

    assert!(result.is_none(), "count mismatch should fail open");
    assert_eq!(
        blocks.len(),
        1,
        "current turn's blocks must be preserved as-is"
    );
    assert!(
        !blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { id, .. } if id == "toolu_2")),
        "stale recorded toolu_2 must not be reinserted"
    );
}

#[tokio::test]
async fn messages_before_last_assistant_are_never_touched() {
    let store = ThinkingRepairStore::new();
    store
        .commit("ns1", "msg_2", vec![thinking("turn 2", "sig_2")])
        .await;

    let mut req = MessageCreateRequest {
        model: "claude-opus-4-5".to_string(),
        max_tokens: 1024,
        messages: vec![
            InputMessage {
                role: Role::User,
                content: Content::Text("first".to_string()),
            },
            InputMessage {
                role: Role::Assistant,
                // Deliberately mutated/corrupt — must be left alone
                // because it is NOT the last assistant message.
                content: Content::Blocks(vec![thinking("mutated turn 1", "sig_1_unknown")]),
            },
            InputMessage {
                role: Role::User,
                content: Content::Text("second".to_string()),
            },
            InputMessage {
                role: Role::Assistant,
                content: Content::Blocks(vec![thinking("turn 2", "sig_2")]),
            },
        ],
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: None,
        extra: serde_json::Map::new(),
    };

    repair_request(&store, "ns1", &mut req).await;

    match &req.messages[1].content {
        Content::Blocks(b) => match &b[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "mutated turn 1");
                assert_eq!(signature.as_deref(), Some("sig_1_unknown"));
            }
            other => panic!("unexpected block: {other:?}"),
        },
        Content::Text(_) => panic!("expected blocks"),
    }
}
