//! Reconstructs full content blocks from an Anthropic SSE event stream and
//! commits them to the [`ThinkingRepairStore`] as ground truth on
//! `message_stop`.
//!
//! Accumulation ([`ThinkingRecorder::observe_event`] / [`ThinkingRecorder::observe_json`])
//! is synchronous and store-independent, since the streaming SSE loop it
//! plugs into (`server/streaming.rs::observe_anthropic_sse_frames`) is
//! itself synchronous. It returns the completed `(msg_id, blocks)` pair on
//! `message_stop` for the caller to commit; `ThinkingRecorder::observe`
//! (test-only) is an async convenience wrapper that commits directly, so
//! tests can feed one event at a time without manually threading the
//! completed pair into `store.commit`.

use anyllm_translate::anthropic::{ContentBlock, Delta, StreamEvent};

#[cfg(test)]
use super::store::ThinkingRepairStore;

/// Defensive cap on content-block index from an SSE stream. A real Anthropic
/// response never has anywhere near this many top-level content blocks; a
/// malformed or adversarial upstream sending a huge `index` must not drive
/// `Vec::resize` into a multi-gigabyte allocation.
const MAX_BLOCKS: usize = 4096;

/// Per-connection accumulator. One instance lives for the lifetime of a
/// single streamed `/v1/messages` response.
#[derive(Default)]
pub struct ThinkingRecorder {
    msg_id: Option<String>,
    blocks: Vec<ContentBlock>,
    /// Accumulated `input_json_delta` text per block index, finalized into
    /// `ToolUse`/`ServerToolUse::input` at `message_stop`.
    partial_json: Vec<String>,
}

impl ThinkingRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one parsed SSE event. Commits the accumulated message to `store`
    /// (scoped to `namespace`) on `message_stop`. Test-only: production code
    /// always goes through `observe_json` (the sync SSE frame loop) or
    /// `observe_event` + a direct `store.commit` (the non-streaming path).
    #[cfg(test)]
    pub async fn observe(
        &mut self,
        event: &StreamEvent,
        store: &ThinkingRepairStore,
        namespace: &str,
    ) {
        if let Some((id, blocks)) = self.observe_event(event) {
            store.commit(namespace, &id, blocks).await;
        }
    }

    /// Parse one raw SSE `data:` payload and feed it. Returns the completed
    /// `(msg_id, blocks)` pair on `message_stop`, for the caller to commit.
    /// Used by the sync SSE frame loop in `server/streaming.rs`, which can't
    /// `.await` a store commit per line.
    pub fn observe_json(&mut self, data: &str) -> Option<(String, Vec<ContentBlock>)> {
        let event: StreamEvent = serde_json::from_str(data).ok()?;
        self.observe_event(&event)
    }

    /// Feed one parsed SSE event. Returns the completed `(msg_id, blocks)`
    /// pair on `message_stop`, for the caller to commit.
    pub fn observe_event(&mut self, event: &StreamEvent) -> Option<(String, Vec<ContentBlock>)> {
        match event {
            StreamEvent::MessageStart { message } => {
                self.msg_id = Some(message.id.clone());
                self.blocks.clear();
                self.partial_json.clear();
                None
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let idx = *index as usize;
                if idx >= MAX_BLOCKS {
                    return None; // ignore: implausible index, avoid unbounded allocation
                }
                self.ensure_len(idx + 1);
                self.blocks[idx] = content_block.clone();
                self.partial_json[idx].clear();
                None
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                self.apply_delta(*index as usize, delta);
                None
            }
            StreamEvent::MessageStop {} => {
                self.finalize_tool_inputs();
                self.msg_id.take().map(|id| {
                    let blocks = std::mem::take(&mut self.blocks);
                    self.partial_json.clear();
                    (id, blocks)
                })
            }
            _ => None,
        }
    }

    fn ensure_len(&mut self, n: usize) {
        if self.blocks.len() < n {
            self.blocks.resize(n, ContentBlock::Unknown);
            self.partial_json.resize(n, String::new());
        }
    }

    fn apply_delta(&mut self, idx: usize, delta: &Delta) {
        if idx >= self.blocks.len() {
            return; // Defensive: content_block_delta before content_block_start.
        }
        match delta {
            Delta::TextDelta { text } => {
                if let ContentBlock::Text { text: t } = &mut self.blocks[idx] {
                    t.push_str(text);
                }
            }
            Delta::ThinkingDelta { thinking } => {
                if let ContentBlock::Thinking { thinking: t, .. } = &mut self.blocks[idx] {
                    t.push_str(thinking);
                }
            }
            Delta::SignatureDelta { signature } => {
                if let ContentBlock::Thinking { signature: sig, .. } = &mut self.blocks[idx] {
                    *sig = Some(signature.clone());
                }
            }
            Delta::InputJsonDelta { partial_json } => {
                self.partial_json[idx].push_str(partial_json);
            }
            Delta::CitationsDelta { .. } | Delta::Unknown => {}
        }
    }

    fn finalize_tool_inputs(&mut self) {
        for (block, buf) in self.blocks.iter_mut().zip(self.partial_json.iter()) {
            if buf.is_empty() {
                continue;
            }
            let input = match block {
                ContentBlock::ToolUse { input, .. } | ContentBlock::ServerToolUse { input, .. } => {
                    Some(input)
                }
                _ => None,
            };
            if let Some(input) = input {
                if let Ok(parsed) = serde_json::from_str(buf) {
                    *input = parsed;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_translate::anthropic::streaming::MessageStartData;
    use anyllm_translate::anthropic::Usage;
    use serde_json::json;

    fn message_start(id: &str) -> StreamEvent {
        StreamEvent::MessageStart {
            message: MessageStartData {
                id: id.to_string(),
                msg_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "claude-opus-4-5".to_string(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 10,
                    ..Default::default()
                },
                created: None,
            },
        }
    }

    #[tokio::test]
    async fn records_thinking_block_with_signature() {
        let store = ThinkingRepairStore::new();
        let mut rec = ThinkingRecorder::new();

        rec.observe(&message_start("msg_1"), &store, "ns1").await;
        rec.observe(
            &StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Thinking {
                    thinking: String::new(),
                    signature: None,
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::ThinkingDelta {
                    thinking: "let me ".to_string(),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::ThinkingDelta {
                    thinking: "think".to_string(),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::SignatureDelta {
                    signature: "sig_xyz".to_string(),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(&StreamEvent::ContentBlockStop { index: 0 }, &store, "ns1")
            .await;
        rec.observe(&StreamEvent::MessageStop {}, &store, "ns1")
            .await;

        let scoped_msg_1 = ThinkingRepairStore::scoped_key("ns1", "msg_1");
        assert_eq!(
            store.lookup_sig("ns1", "sig_xyz").await,
            Some((scoped_msg_1.clone(), 0))
        );
        let recorded = store.message(&scoped_msg_1).await.unwrap();
        match &recorded[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "let me think");
                assert_eq!(signature.as_deref(), Some("sig_xyz"));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[tokio::test]
    async fn finalizes_tool_use_input_from_partial_json() {
        let store = ThinkingRepairStore::new();
        let mut rec = ThinkingRecorder::new();

        rec.observe(&message_start("msg_1"), &store, "ns1").await;
        rec.observe(
            &StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({}),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::InputJsonDelta {
                    partial_json: "{\"city\":".to_string(),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::InputJsonDelta {
                    partial_json: "\"nyc\"}".to_string(),
                },
            },
            &store,
            "ns1",
        )
        .await;
        rec.observe(&StreamEvent::MessageStop {}, &store, "ns1")
            .await;

        let scoped_msg_1 = ThinkingRepairStore::scoped_key("ns1", "msg_1");
        let recorded = store.message(&scoped_msg_1).await.unwrap();
        match &recorded[0] {
            ContentBlock::ToolUse { input, .. } => assert_eq!(input, &json!({"city": "nyc"})),
            other => panic!("unexpected block: {other:?}"),
        }
        assert_eq!(
            store.owner_of_tool_use("ns1", "toolu_1").await,
            Some(scoped_msg_1)
        );
    }

    #[test]
    fn observe_json_returns_completed_message_on_message_stop() {
        let mut rec = ThinkingRecorder::new();
        assert!(rec
            .observe_json(r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-opus-4-5","usage":{"input_tokens":1,"output_tokens":0}}}"#)
            .is_none());
        assert!(rec
            .observe_json(r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#)
            .is_none());
        assert!(rec
            .observe_json(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#)
            .is_none());
        let done = rec.observe_json(r#"{"type":"message_stop"}"#);
        let (id, blocks) = done.expect("message_stop should return the completed message");
        assert_eq!(id, "msg_1");
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hi"));
    }

    #[test]
    fn observe_json_ignores_malformed_data() {
        let mut rec = ThinkingRecorder::new();
        assert!(rec.observe_json("not json").is_none());
    }
}
