use crate::anthropic;
use crate::mapping::warnings::TranslationWarnings;

/// Extract system prompt text from Anthropic's System type.
/// Warns if cache_control is present (no equivalent in downstream APIs).
pub fn extract_system_text(system: &anthropic::System) -> String {
    if let anthropic::System::Blocks(blocks) = system {
        if blocks.iter().any(|b| b.cache_control.is_some()) {
            tracing::warn!("cache_control on system blocks dropped: no downstream equivalent");
        }
    }
    match system {
        anthropic::System::Text(s) => s.clone(),
        anthropic::System::Blocks(blocks) => blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Compute degradation warnings for an Anthropic request without performing translation.
///
/// Returns a `TranslationWarnings` value listing every feature that will be silently
/// dropped or degraded when this request is translated to an OpenAI request.
/// The proxy injects these as an `x-anyllm-degradation` response header so clients
/// can detect silent drops without inspecting server logs.
pub fn compute_request_warnings(req: &anthropic::MessageCreateRequest) -> TranslationWarnings {
    let mut w = TranslationWarnings::default();
    if req.top_k.is_some() {
        w.add("top_k");
    }
    if req.thinking.is_some() {
        w.add("thinking_config");
    }
    if let Some(ref seqs) = req.stop_sequences {
        if seqs.len() > 4 {
            w.add("stop_sequences_truncated");
        }
    }
    if let Some(anthropic::System::Blocks(blocks)) = &req.system {
        if blocks.iter().any(|b| b.cache_control.is_some()) {
            w.add("cache_control");
        }
    }
    let has_document = req.messages.iter().any(|msg| match &msg.content {
        anthropic::Content::Blocks(blocks) => blocks
            .iter()
            .any(|b| matches!(b, anthropic::ContentBlock::Document { .. })),
        _ => false,
    });
    if has_document {
        w.add("document_blocks");
    }
    w
}
