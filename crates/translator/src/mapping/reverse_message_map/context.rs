// Request-local translation context: sanitized <-> original Anthropic tool
// name mapping used to round-trip OpenAI tool names through Anthropic.

use crate::openai;
use std::collections::BTreeMap;

/// Request-local metadata needed to round-trip Anthropic-compatible tool names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicTranslationContext {
    original_to_sanitized_tool_names: BTreeMap<String, String>,
    sanitized_to_original_tool_names: BTreeMap<String, String>,
}

impl AnthropicTranslationContext {
    pub fn from_openai_request(req: &openai::ChatCompletionRequest) -> Self {
        let mut ctx = Self::default();

        if let Some(tools) = &req.tools {
            for tool in tools {
                ctx.register_tool_name(&tool.function.name);
            }
        }
        if let Some(openai::ChatToolChoice::Named(named)) = &req.tool_choice {
            ctx.register_tool_name(&named.function.name);
        }
        for message in &req.messages {
            if let Some(tool_calls) = &message.tool_calls {
                for tool_call in tool_calls {
                    ctx.register_tool_name(&tool_call.function.name);
                }
            }
        }

        ctx
    }

    pub fn sanitized_tool_name(&self, name: &str) -> String {
        self.original_to_sanitized_tool_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn original_tool_name(&self, name: &str) -> String {
        self.sanitized_to_original_tool_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn register_tool_name(&mut self, original: &str) -> String {
        if let Some(existing) = self.original_to_sanitized_tool_names.get(original) {
            return existing.clone();
        }

        let base = basic_sanitize_anthropic_tool_name(original);
        let mut candidate = base.clone();
        let mut suffix_index = 2usize;
        while self
            .sanitized_to_original_tool_names
            .contains_key(&candidate)
        {
            let suffix = format!("_{suffix_index}");
            let keep = 128usize.saturating_sub(suffix.len());
            candidate = format!("{}{}", &base[..base.len().min(keep)], suffix);
            suffix_index += 1;
        }

        self.original_to_sanitized_tool_names
            .insert(original.to_string(), candidate.clone());
        self.sanitized_to_original_tool_names
            .insert(candidate.clone(), original.to_string());
        candidate
    }
}

fn basic_sanitize_anthropic_tool_name(original: &str) -> String {
    let mut sanitized: String = original
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(128)
        .collect();

    if sanitized.is_empty() {
        sanitized = "tool".to_string();
    }
    sanitized
}
