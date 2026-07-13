//! Tiered, deterministic, prefix-preserving repair of the last assistant
//! message's thinking blocks, verified against recorded ground truth.
//!
//! Only ever touches `req.messages[last_assistant_idx]`. Everything before
//! it (the cached prompt-cache prefix) is left byte-identical.
//!
//! Tiers:
//!   0. Block matches the recorded original exactly -> keep as-is.
//!   1. Signature matches a recorded block but the text differs (e.g. a
//!      client merged two thinking texts under one signature) -> restore
//!      the recorded original bytes.
//!   2. Block belongs to a different recorded message than the one owning
//!      this turn's tool_use ids ("intruder"), or its signature is unknown
//!      while the owning message is known -> drop it.
//!
//! Unknown signature with no resolvable owner: fail open, pass through.
//!
//! A final pass reinserts thinking/redacted_thinking blocks the client lost
//! entirely (e.g. Claude Code not persisting `redacted_thinking` to JSONL),
//! restoring the recorded order, but only when nothing else needed
//! restoring or dropping this turn (a conservative, low-risk trigger).

use anyllm_translate::anthropic::{ContentBlock, InputMessage, MessageCreateRequest, Role};

use super::store::ThinkingRepairStore;

/// Repair the last assistant message in `req.messages` against `store`.
/// `namespace` scopes every store lookup to the calling backend/tenant (see
/// `ThinkingRepairStore`'s doc comment) so this can never resolve or restore
/// content recorded from a different tenant's conversation.
/// Returns a short description of what changed, or `None` if the request
/// was left untouched.
pub async fn repair_request(
    store: &ThinkingRepairStore,
    namespace: &str,
    req: &mut MessageCreateRequest,
) -> Option<String> {
    let last_idx = req
        .messages
        .iter()
        .rposition(|m: &InputMessage| m.role == Role::Assistant)?;

    let content = match &mut req.messages[last_idx].content {
        anyllm_translate::anthropic::Content::Blocks(blocks) => std::mem::take(blocks),
        anyllm_translate::anthropic::Content::Text(_) => return None,
    };

    // Which recorded message "owns" this turn? Resolved via tool_use ids,
    // which the following user message's tool_results must answer.
    let mut owner: Option<String> = None;
    for block in &content {
        if let ContentBlock::ToolUse { id, .. } = block {
            if let Some(msg_id) = store.owner_of_tool_use(namespace, id).await {
                owner = Some(msg_id);
                break;
            }
        }
    }

    // Fetch the owner's recorded blocks ONCE for this call, not once per
    // thinking-ish block (`owner` is resolved above and never changes for
    // the rest of this function, so its record can't change underneath us
    // either). `None` here means either there's no owner, or the owner's
    // by_msg entry was independently evicted (moka's three indices evict on
    // their own schedules) -- in the latter case we can't verify anything
    // against it, so every branch below that consults `owner_record` treats
    // a `None` as "unverifiable" and fails OPEN, never as grounds to drop.
    let owner_record = match &owner {
        Some(own) => store.message(own).await,
        None => None,
    };

    let mut restored = 0usize;
    let mut dropped = 0usize;
    let mut rebuilt: Vec<ContentBlock> = Vec::with_capacity(content.len());

    for block in content {
        match &block {
            ContentBlock::Thinking { signature, .. } => {
                let sig = signature.clone().unwrap_or_default();
                match store.lookup_sig(namespace, &sig).await {
                    Some((msg_id, idx)) => {
                        if let Some(own) = &owner {
                            if &msg_id != own {
                                dropped += 1;
                                continue;
                            }
                        }
                        // When `owner` is Some, the guard above guarantees
                        // `msg_id == owner`, so `owner_record` already holds
                        // this message -- reuse it instead of hitting by_msg
                        // again. When `owner` is None, `msg_id` may be a
                        // different message entirely, so it must be looked
                        // up on its own.
                        let rec = if owner.is_some() {
                            owner_record.clone()
                        } else {
                            store.message(&msg_id).await
                        };
                        let original = rec.as_ref().and_then(|rec| rec.get(idx).cloned());
                        if let Some(orig) = original {
                            if !thinking_eq(&orig, &block) {
                                restored += 1;
                                rebuilt.push(orig);
                                continue;
                            }
                        }
                        rebuilt.push(block); // Tier 0: matches ground truth.
                    }
                    None => {
                        // Unknown signature: recorded before the proxy saw
                        // it, or garbage -- OR the by_sig index independently
                        // evicted this entry while by_tool_use/by_msg still
                        // resolve the owning message (three separate moka
                        // caches evict on their own schedules). Before
                        // dropping, check the owner's own recorded blocks
                        // directly: if this exact block is there, it's
                        // legitimate regardless of what by_sig says.
                        match (&owner, &owner_record) {
                            (Some(_), Some(rec)) => {
                                if rec.iter().any(|b| thinking_eq(b, &block)) {
                                    rebuilt.push(block);
                                } else {
                                    dropped += 1;
                                }
                            }
                            // Owner is known but its record was ALSO evicted
                            // (by_sig and by_msg both missed for the same
                            // message) -- there is no ground truth left to
                            // check this block against either way. Fail
                            // open and keep it rather than drop a possibly
                            // legitimate block just because eviction beat us
                            // to it.
                            (Some(_), None) => rebuilt.push(block),
                            (None, _) => rebuilt.push(block),
                        }
                    }
                }
            }
            ContentBlock::RedactedThinking { .. } => {
                if owner.is_some() {
                    let ok = match &owner_record {
                        Some(rec) => rec.iter().any(|b| thinking_eq(b, &block)),
                        None => true, // no record -> fail open
                    };
                    if ok {
                        rebuilt.push(block);
                    } else {
                        dropped += 1;
                    }
                } else {
                    rebuilt.push(block);
                }
            }
            _ => rebuilt.push(block),
        }
    }

    // Reinsert recorded thinking/redacted_thinking the client lost entirely,
    // but only when this turn otherwise validated cleanly (nothing restored
    // or dropped) — a conservative trigger to avoid compounding repairs.
    // Every thinking-ish block already in `rebuilt` was already validated
    // against the owner's record above (Tier 0 signature match, or the
    // RedactedThinking existence check), so it is always a subset of `want`;
    // the only question is whether any are missing.
    if dropped == 0 && restored == 0 && owner.is_some() {
        if let Some(rec) = owner_record.as_ref() {
            let have_count = rebuilt.iter().filter(|b| is_thinkingish(b)).count();
            let want_count = rec.iter().filter(|b| is_thinkingish(b)).count();
            if have_count < want_count {
                // Reinsert missing blocks in their RECORDED positions,
                // interleaved with the current turn's non-thinking blocks
                // in order -- not concatenated as all-thinking-then-rest,
                // which would silently reorder interleaved thinking/tool_use
                // history that never actually happened. Only safe when the
                // current turn's non-thinking blocks line up 1:1 with the
                // recorded ones: a count mismatch means the client sent a
                // materially different turn, and interleaving positionally
                // would either drop current-turn content or reintroduce
                // stale recorded content, so fall back to the current turn
                // unmodified rather than guess.
                let mut non_thinking = rebuilt.iter().filter(|b| !is_thinkingish(b));
                let rec_non_thinking_count = rec.iter().filter(|b| !is_thinkingish(b)).count();
                if non_thinking.by_ref().count() != rec_non_thinking_count {
                    req.messages[last_idx].content =
                        anyllm_translate::anthropic::Content::Blocks(rebuilt);
                    return None;
                }
                let mut non_thinking = rebuilt.into_iter().filter(|b| !is_thinkingish(b));
                let merged: Vec<ContentBlock> = rec
                    .iter()
                    .map(|b| {
                        if is_thinkingish(b) {
                            b.clone()
                        } else {
                            non_thinking.next().unwrap_or_else(|| b.clone())
                        }
                    })
                    .collect();
                let n = merged.len();
                req.messages[last_idx].content =
                    anyllm_translate::anthropic::Content::Blocks(merged);
                return Some(format!(
                    "reinserted lost thinking blocks ({n} blocks total)"
                ));
            }
        }
    }

    req.messages[last_idx].content = anyllm_translate::anthropic::Content::Blocks(rebuilt);
    if restored + dropped > 0 {
        Some(format!(
            "restored {restored}, dropped {dropped} thinking block(s)"
        ))
    } else {
        None
    }
}

fn is_thinkingish(b: &ContentBlock) -> bool {
    matches!(
        b,
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
    )
}

/// Structural equality for the two thinking-ish variants only. Deliberately
/// not a blanket `PartialEq` on `ContentBlock` (a type shared across the
/// whole workspace) — this repair module is the only place that needs it.
fn thinking_eq(a: &ContentBlock, b: &ContentBlock) -> bool {
    match (a, b) {
        (
            ContentBlock::Thinking {
                thinking: t1,
                signature: s1,
            },
            ContentBlock::Thinking {
                thinking: t2,
                signature: s2,
            },
        ) => t1 == t2 && s1 == s2,
        (
            ContentBlock::RedactedThinking { data: d1 },
            ContentBlock::RedactedThinking { data: d2 },
        ) => d1 == d2,
        _ => false,
    }
}

#[cfg(test)]
#[path = "repair/tests.rs"]
mod tests;
