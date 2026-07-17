use crate::anthropic;
use crate::mapping::tools_map;
use crate::openai;
use crate::util;

/// Proxy-internal marker key placed on an Anthropic request's `extra` map to
/// signal that the original caller omitted `max_tokens` and the request targets
/// an OpenAI-compatible backend (where max_tokens is optional). When present,
/// [`anthropic_to_openai_request`] clears the placeholder max_tokens on the
/// outbound OpenAI request so the backend applies its own default, and removes
/// the marker so it never leaks upstream. The proxy sets this authoritatively;
/// see `chat_completions/handler.rs`.
pub const OMIT_MAX_TOKENS_MARKER: &str = "__anyllm_omit_max_tokens";

/// Convert an Anthropic MessageCreateRequest to an OpenAI ChatCompletionRequest.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
pub fn anthropic_to_openai_request(
    req: &anthropic::MessageCreateRequest,
) -> openai::ChatCompletionRequest {
    let mut messages = Vec::new();

    if let Some(ref system) = req.system {
        let text = super::extract_system_text(system);
        // Uses System role instead of Developer for backward compat with
        // local LLMs (vLLM, Ollama, llama-server) that don't recognize
        // "developer". Trade-off: OpenAI o1/o3 require "developer" and
        // reject "system". We chose broader compat over o-series support
        // because most proxy users target GPT-4o or local models.
        messages.push(openai::ChatMessage {
            role: openai::ChatRole::System,
            content: Some(openai::ChatContent::Text(text)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            refusal: None,
            reasoning_content: None,
            thinking_blocks: None,
        });
    }

    for msg in &req.messages {
        convert_anthropic_message(msg, &mut messages);
    }

    let tools = req
        .tools
        .as_ref()
        .map(|t| tools_map::anthropic_tools_to_openai(t));

    let tool_choice = req
        .tool_choice
        .as_ref()
        .map(tools_map::anthropic_tool_choice_to_openai);

    // Map disable_parallel_tool_use to OpenAI parallel_tool_calls.
    // Compat spec: "Fully supported". See: https://docs.anthropic.com/en/api/openai-sdk#tools--functions-fields
    let parallel_tool_calls = match req.tool_choice.as_ref() {
        Some(anthropic::ToolChoice::Auto {
            disable_parallel_tool_use: Some(true),
        })
        | Some(anthropic::ToolChoice::Any {
            disable_parallel_tool_use: Some(true),
        }) => Some(false),
        _ => None,
    };

    // Map metadata.user_id to OpenAI user field.
    // Compat spec: user is "Ignored", but we forward it for traceability.
    // See: https://docs.anthropic.com/en/api/openai-sdk#simple-fields
    let user = req.metadata.as_ref().and_then(|m| m.user_id.clone());
    if req.top_k.is_some() {
        tracing::warn!("top_k parameter dropped: no OpenAI equivalent");
    }
    // Map Anthropic thinking config to OpenAI reasoning_effort (via extra).
    // Thinking content blocks in messages are separately mapped to reasoning_content.
    // Thresholds (4 k / 16 k tokens) are proxy-inferred approximations; OpenAI does not
    // document exact token counts for each effort level. Chosen to match common usage:
    //   low  ≈ quick CoT drafts (< 4 k), medium ≈ standard reasoning, high ≈ extended thinking.
    let reasoning_effort = match &req.thinking {
        Some(crate::anthropic::ThinkingConfig::Enabled { budget_tokens }) => {
            let effort = if *budget_tokens < 4_000 {
                "low"
            } else if *budget_tokens < 16_000 {
                "medium"
            } else {
                "high"
            };
            tracing::info!(
                budget_tokens,
                reasoning_effort = effort,
                "thinking config mapped to reasoning_effort"
            );
            Some(effort)
        }
        _ => None,
    };

    // OpenAI caps stop sequences at 4; empty array is invalid (requires 1-4 elements)
    let stop = req.stop_sequences.as_ref().and_then(|seqs| {
        if seqs.is_empty() {
            return None;
        }
        if seqs.len() > 4 {
            tracing::warn!(
                count = seqs.len(),
                "stop_sequences truncated from {} to 4 (OpenAI limit)",
                seqs.len()
            );
        }
        let capped: Vec<String> = seqs.iter().take(4).cloned().collect();
        Some(if capped.len() == 1 {
            openai::Stop::Single(capped.into_iter().next().unwrap())
        } else {
            openai::Stop::Multiple(capped)
        })
    });

    let mut oai_req = openai::ChatCompletionRequest {
        model: req.model.clone(),
        messages,
        // Default: set both for local LLM compat (vLLM, ollama only
        // recognize max_tokens). Overridden below for o-series models.
        max_tokens: Some(req.max_tokens),
        max_completion_tokens: Some(req.max_tokens),
        // Compat spec: "Between 0 and 1 (inclusive). Values greater than 1 are capped at 1."
        // See: https://docs.anthropic.com/en/api/openai-sdk#simple-fields
        temperature: req.temperature.map(|t| t.clamp(0.0, 1.0)),
        top_p: req.top_p,
        stop,
        tools,
        tool_choice,
        stream: req.stream,
        // Required for the streaming translator: without include_usage=true,
        // OpenAI omits the final usage chunk and we cannot report token counts
        // back to the Anthropic client. Local LLMs that don't support
        // stream_options may reject this with 400.
        stream_options: if req.stream == Some(true) {
            Some(openai::StreamOptions {
                include_usage: true,
            })
        } else {
            None
        },
        presence_penalty: None,
        frequency_penalty: None,
        response_format: None,
        user,
        parallel_tool_calls,
        extra: req.extra.clone(),
    };

    // Inject reasoning_effort if derived from thinking config and not already set.
    if let Some(effort) = reasoning_effort {
        oai_req
            .extra
            .entry("reasoning_effort")
            .or_insert_with(|| serde_json::Value::String(effort.to_owned()));
    }

    // Anthropic API returns single completions only; strip n to avoid
    // wasting tokens on choices that get discarded (only choices[0] is used).
    if let Some(n_val) = oai_req.extra.remove("n") {
        if n_val != serde_json::Value::Number(1.into()) {
            tracing::warn!(n = %n_val, "n parameter stripped: Anthropic API returns single completions only");
        }
    }

    // o-series reasoning models require "developer" role instead of "system",
    // reject max_tokens (use max_completion_tokens instead), and reject
    // temperature/top_p — all variants, including GA releases.
    if is_o_series_model(&oai_req.model) {
        oai_req.max_tokens = None;
        oai_req.temperature = None;
        oai_req.top_p = None;
        for msg in &mut oai_req.messages {
            if msg.role == openai::ChatRole::System {
                msg.role = openai::ChatRole::Developer;
            }
        }
    }

    // When a specific tool is forced, enable OpenAI strict structured outputs for it.
    // This guarantees the returned JSON exactly matches the schema.
    if let Some(forced_name) = req.tool_choice.as_ref().and_then(extract_forced_tool_name) {
        if let Some(ref mut tools) = oai_req.tools {
            apply_strict_mode_to_tool(tools, &forced_name);
        }
    }

    // Proxy-internal marker (see OMIT_MAX_TOKENS_MARKER): the caller omitted
    // max_tokens for an OpenAI-compat backend, where it is optional. Clear the
    // placeholder so the backend uses its own default (e.g. LM Studio's 8192),
    // and strip the marker so it never leaks upstream.
    if oai_req.extra.remove(OMIT_MAX_TOKENS_MARKER).is_some() {
        oai_req.max_tokens = None;
        oai_req.max_completion_tokens = None;
    }

    oai_req
}

/// Extract the forced tool name from an Anthropic ToolChoice, if any.
fn extract_forced_tool_name(tc: &anthropic::ToolChoice) -> Option<String> {
    match tc {
        anthropic::ToolChoice::Tool { name } => Some(name.clone()),
        _ => None,
    }
}

/// Set strict=true and normalize the parameter schema for the named tool.
/// Other tools in the vec are left unchanged.
fn apply_strict_mode_to_tool(tools: &mut [openai::ChatTool], forced_name: &str) {
    for tool in tools.iter_mut() {
        if tool.function.name == forced_name {
            tool.function.strict = Some(true);
            if let Some(params) = tool.function.parameters.take() {
                tool.function.parameters = Some(tools_map::normalize_schema_for_strict(params));
            }
            // Tool names are unique; stop after the first match.
            break;
        }
    }
}

/// Returns true if the model name matches an OpenAI o-series reasoning model
/// (o1, o2, o3, o4, o5, o10, etc.). Does not match "gpt-4o" where 'o' is a suffix.
/// Pattern: starts with 'o' or 'O', then one or more ASCII digits, then end-of-string or '-'.
pub(crate) fn is_o_series_model(model: &str) -> bool {
    // Matches: o1, o2, o3, o10, o1-mini, O4-preview, etc.
    // Rejects: gpt-4o (suffix 'o'), openai-x, o-preview (no digit after 'o').
    let bytes = model.as_bytes();
    if bytes.is_empty() || !bytes[0].eq_ignore_ascii_case(&b'o') {
        return false;
    }
    // Must have at least one digit after the 'o'.
    if bytes.len() < 2 || !bytes[1].is_ascii_digit() {
        return false;
    }
    // Skip remaining digits.
    let after_digits = bytes[1..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map(|p| 1 + p)
        .unwrap_or(bytes.len());
    // After the digits: either end-of-string or a '-'.
    after_digits == bytes.len() || bytes[after_digits] == b'-'
}

/// Convert a single Anthropic InputMessage into one or more OpenAI ChatMessages.
/// An assistant message with tool_use blocks produces tool_calls.
/// A user message with tool_result blocks produces OpenAI tool-role messages.
fn convert_anthropic_message(msg: &anthropic::InputMessage, out: &mut Vec<openai::ChatMessage>) {
    let role = match msg.role {
        anthropic::Role::User => openai::ChatRole::User,
        anthropic::Role::Assistant => openai::ChatRole::Assistant,
    };

    match &msg.content {
        anthropic::Content::Text(text) => {
            out.push(openai::ChatMessage {
                role,
                content: Some(openai::ChatContent::Text(text.clone())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            });
        }
        anthropic::Content::Blocks(blocks) => {
            if msg.role == anthropic::Role::Assistant {
                convert_assistant_blocks(blocks, out);
            } else {
                convert_user_blocks(blocks, out);
            }
        }
    }
}

/// Assistant blocks: text parts become content, tool_use blocks become tool_calls.
fn convert_assistant_blocks(
    blocks: &[anthropic::ContentBlock],
    out: &mut Vec<openai::ChatMessage>,
) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut thinking_parts = Vec::new();
    let mut thinking_blocks = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(openai::ToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: openai::FunctionCall {
                        name: name.clone(),
                        arguments: util::json::value_to_json_string(input),
                    },
                });
            }
            anthropic::ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                thinking_parts.push(thinking.clone());
                thinking_blocks.push(openai::ThinkingBlock::Thinking {
                    thinking: thinking.clone(),
                    signature: signature.clone(),
                });
            }
            anthropic::ContentBlock::RedactedThinking { data } => {
                thinking_blocks
                    .push(openai::ThinkingBlock::RedactedThinking { data: data.clone() });
            }
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(openai::ChatContent::Text(text_parts.join("")))
    };

    let reasoning_content = if thinking_parts.is_empty() {
        None
    } else {
        Some(thinking_parts.join(""))
    };

    out.push(openai::ChatMessage {
        role: openai::ChatRole::Assistant,
        content,
        name: None,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
        refusal: None,
        reasoning_content,
        thinking_blocks: if thinking_blocks.is_empty() {
            None
        } else {
            Some(thinking_blocks)
        },
    });
}

/// Resolve an Anthropic ImageSource to a URL string (data URI or direct URL).
fn image_source_to_url(source: &anthropic::messages::ImageSource) -> Option<String> {
    if let Some(ref url) = source.url {
        Some(url.clone())
    } else if let Some(ref data) = source.data {
        let mt = source.media_type.as_deref().unwrap_or("image/png");
        Some(format!("data:{};base64,{}", mt, data))
    } else {
        None
    }
}

/// Simplify a Vec of content parts: use plain Text when there's a single text part,
/// multipart array otherwise. Moves data out of the Vec to avoid cloning.
fn simplify_content_parts(mut parts: Vec<openai::ChatContentPart>) -> openai::ChatContent {
    if parts.len() == 1 {
        match parts.remove(0) {
            openai::ChatContentPart::Text { text } => openai::ChatContent::Text(text),
            other => openai::ChatContent::Parts(vec![other]),
        }
    } else {
        openai::ChatContent::Parts(parts)
    }
}

/// User blocks: text/image parts become content, tool_result blocks become
/// separate OpenAI tool-role messages.
fn convert_user_blocks(blocks: &[anthropic::ContentBlock], out: &mut Vec<openai::ChatMessage>) {
    let mut content_parts: Vec<openai::ChatContentPart> = Vec::new();
    let mut tool_results: Vec<(String, Vec<openai::ChatContentPart>)> = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                content_parts.push(openai::ChatContentPart::Text { text: text.clone() });
            }
            anthropic::ContentBlock::Image { source } => {
                if let Some(url) = image_source_to_url(source) {
                    content_parts.push(openai::ChatContentPart::ImageUrl {
                        image_url: openai::chat_completions::ImageUrl { url, detail: None },
                    });
                }
            }
            anthropic::ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let mut parts: Vec<openai::ChatContentPart> = Vec::new();
                match content {
                    Some(anthropic::messages::ToolResultContent::Text(s)) => {
                        parts.push(openai::ChatContentPart::Text { text: s.clone() });
                    }
                    Some(anthropic::messages::ToolResultContent::Blocks(inner)) => {
                        for b in inner {
                            match b {
                                anthropic::ContentBlock::Text { text } => {
                                    parts
                                        .push(openai::ChatContentPart::Text { text: text.clone() });
                                }
                                anthropic::ContentBlock::Image { source } => {
                                    if let Some(url) = image_source_to_url(source) {
                                        parts.push(openai::ChatContentPart::ImageUrl {
                                            image_url: openai::chat_completions::ImageUrl {
                                                url,
                                                detail: None,
                                            },
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    None => {}
                };
                // Anthropic's is_error flag has no direct OpenAI equivalent.
                // We surface it as a text prefix so the backend model sees the
                // error context in message history.
                if *is_error == Some(true) {
                    // Prefix the first text part (or add one) with "Error: "
                    if let Some(openai::ChatContentPart::Text { ref mut text }) = parts
                        .iter_mut()
                        .find(|p| matches!(p, openai::ChatContentPart::Text { .. }))
                    {
                        *text = format!("Error: {}", text);
                    } else {
                        parts.insert(
                            0,
                            openai::ChatContentPart::Text {
                                text: "Error".to_string(),
                            },
                        );
                    }
                }
                // Use empty text if no content was provided
                if parts.is_empty() {
                    parts.push(openai::ChatContentPart::Text {
                        text: String::new(),
                    });
                }
                tool_results.push((tool_use_id.clone(), parts));
            }
            anthropic::ContentBlock::Document { source, title } => {
                // OpenAI Chat Completions has no inline document support;
                // the Responses API (input_file.file_data) would be needed
                // for full fidelity. Degrade to a text note so the model
                // still sees that a document was attached.
                let label = title.as_deref().unwrap_or("document");
                tracing::warn!(
                    label = label,
                    "document block degraded to text note: no OpenAI Chat Completions equivalent"
                );
                let note = format!(
                    "[Attached {}: {} ({} bytes base64)]",
                    label,
                    source.media_type,
                    source.data.len()
                );
                content_parts.push(openai::ChatContentPart::Text { text: note });
            }
            // ToolUse and Thinking blocks don't appear in user messages; ignore if present
            anthropic::ContentBlock::ToolUse { .. }
            | anthropic::ContentBlock::Thinking { .. }
            | anthropic::ContentBlock::RedactedThinking { .. } => {}
            // Wildcard to handle new/unknown variants (like ServerToolUse, WebSearchToolResult, WebFetchToolResult, Unknown)
            _ => {}
        }
    }

    // Emit tool results before user content: OpenAI enforces strict turn
    // ordering where Tool messages must immediately follow the Assistant
    // message that produced the tool_calls. Violating this causes 400s.
    for (tool_call_id, parts) in tool_results {
        let content = Some(simplify_content_parts(parts));
        out.push(openai::ChatMessage {
            role: openai::ChatRole::Tool,
            content,
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
            refusal: None,
            reasoning_content: None,
            thinking_blocks: None,
        });
    }

    // Emit user content message after tool results
    if !content_parts.is_empty() {
        let content = Some(simplify_content_parts(content_parts));
        out.push(openai::ChatMessage {
            role: openai::ChatRole::User,
            content,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            refusal: None,
            reasoning_content: None,
            thinking_blocks: None,
        });
    }
}
