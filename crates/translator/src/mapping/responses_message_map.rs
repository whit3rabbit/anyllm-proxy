// Phase 22: Anthropic <-> OpenAI Responses API message mapping
//
// Pure translation between Anthropic Messages API and OpenAI Responses API.
// The Responses API uses `input` (text or items) instead of `messages[]`,
// `instructions` instead of system messages, and `output[]` instead of `choices[]`.

use crate::anthropic;
use crate::mapping::message_map::extract_system_text;
use crate::openai::responses::{ResponsesInput, ResponsesRequest, ResponsesResponse};
use crate::util;
use serde_json::{json, Value};

/// Convert an Anthropic MessageCreateRequest to an OpenAI Responses API request.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI Responses: <https://platform.openai.com/docs/api-reference/responses/create>
pub fn anthropic_to_responses_request(req: &anthropic::MessageCreateRequest) -> ResponsesRequest {
    let instructions = req.system.as_ref().map(extract_system_text);

    let input = build_input_items(&req.messages);

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| {
                let mut tool = json!({
                    "type": "function",
                    "name": t.name,
                    "parameters": t.input_schema,
                    "strict": false,
                });
                if let Some(ref desc) = t.description {
                    tool["description"] = json!(desc);
                }
                tool
            })
            .collect()
    });

    if req.top_k.is_some() {
        tracing::warn!("top_k parameter dropped: no OpenAI equivalent");
    }
    if req.thinking.is_some() {
        tracing::warn!("thinking config stripped: no OpenAI equivalent");
    }
    if req.metadata.is_some() {
        tracing::warn!("metadata dropped: no direct OpenAI Responses API equivalent");
    }

    let mut extra = serde_json::Map::new();
    if let Some(top_p) = req.top_p {
        extra.insert("top_p".into(), json!(top_p));
    }
    if let Some(ref tc) = req.tool_choice {
        let mapped = match tc {
            anthropic::ToolChoice::Auto => json!("auto"),
            anthropic::ToolChoice::Any => json!("required"),
            anthropic::ToolChoice::None => json!("none"),
            anthropic::ToolChoice::Tool { name } => json!({
                "type": "function",
                "name": name,
            }),
        };
        extra.insert("tool_choice".into(), mapped);
    }

    if let Some(ref seqs) = req.stop_sequences {
        if seqs.len() > 4 {
            tracing::warn!(
                count = seqs.len(),
                "stop_sequences truncated from {} to 4 (OpenAI limit)",
                seqs.len()
            );
        }
        let capped: Vec<&str> = seqs.iter().take(4).map(|s| s.as_str()).collect();
        extra.insert("stop".into(), json!(capped));
    }

    ResponsesRequest {
        model: req.model.clone(),
        input,
        instructions,
        max_output_tokens: Some(req.max_tokens),
        temperature: req.temperature.map(|t| t.clamp(0.0, 1.0)),
        tools,
        stream: req.stream,
        extra,
    }
}

/// Build Responses API input items from Anthropic messages.
///
/// The Responses API models tool calls as first-class items in the input
/// array, not as message content blocks. Anthropic tool_use blocks become
/// function_call items at the root level; tool_result blocks become
/// function_call_output items. This flattened structure is required by the
/// Responses API schema.
fn build_input_items(messages: &[anthropic::InputMessage]) -> ResponsesInput {
    let mut items: Vec<Value> = Vec::new();

    for msg in messages {
        let role = match msg.role {
            anthropic::Role::User => "user",
            anthropic::Role::Assistant => "assistant",
        };

        match &msg.content {
            anthropic::Content::Text(text) => {
                items.push(json!({
                    "type": "message",
                    "role": role,
                    "content": [{"type": "input_text", "text": text}],
                }));
            }
            anthropic::Content::Blocks(blocks) => {
                convert_blocks_to_items(blocks, role, &mut items);
            }
        }
    }

    ResponsesInput::Items(items)
}

/// Convert Anthropic content blocks into Responses API input items.
///
/// Text/image blocks are grouped into a message item.
/// Tool results become separate `function_call_output` items.
/// Tool use blocks (in assistant messages) become `function_call` items.
fn convert_blocks_to_items(blocks: &[anthropic::ContentBlock], role: &str, items: &mut Vec<Value>) {
    let mut content_parts: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut tool_results: Vec<Value> = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                content_parts.push(json!({"type": "input_text", "text": text}));
            }
            anthropic::ContentBlock::Image { source } => {
                if let Some(ref url) = source.url {
                    content_parts.push(json!({
                        "type": "input_image",
                        "image_url": url,
                    }));
                } else if let Some(ref data) = source.data {
                    let mt = source.media_type.as_deref().unwrap_or("image/png");
                    content_parts.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{};base64,{}", mt, data),
                    }));
                }
            }
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(json!({
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": util::json::value_to_json_string(input),
                }));
            }
            anthropic::ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                let output = tool_result_to_string(content.as_ref());
                tool_results.push(json!({
                    "type": "function_call_output",
                    "call_id": tool_use_id,
                    "output": output,
                }));
            }
            anthropic::ContentBlock::Document { .. } => {
                tracing::warn!("document block degraded to text note in Responses API translation");
                content_parts.push(json!({
                    "type": "input_text",
                    "text": "[Document content not supported in translation]",
                }));
            }
            anthropic::ContentBlock::Thinking { .. }
            | anthropic::ContentBlock::RedactedThinking { .. } => {
                // Silently dropped, same as Chat Completions path
            }
        }
    }

    if !content_parts.is_empty() {
        items.push(json!({
            "type": "message",
            "role": role,
            "content": content_parts,
        }));
    }

    items.extend(tool_calls);
    items.extend(tool_results);
}

/// Extract text from an Anthropic tool result content.
fn tool_result_to_string(content: Option<&anthropic::messages::ToolResultContent>) -> String {
    match content {
        None => String::new(),
        Some(anthropic::messages::ToolResultContent::Text(t)) => t.clone(),
        Some(anthropic::messages::ToolResultContent::Blocks(blocks)) => {
            let mut parts = Vec::new();
            for b in blocks {
                if let anthropic::ContentBlock::Text { text } = b {
                    parts.push(text.as_str());
                }
            }
            parts.join("\n")
        }
    }
}

/// Convert an OpenAI Responses API response to an Anthropic MessageResponse.
///
/// OpenAI Responses: <https://platform.openai.com/docs/api-reference/responses/object>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn responses_to_anthropic_response(
    resp: &ResponsesResponse,
    original_model: &str,
) -> anthropic::MessageResponse {
    let mut content = Vec::new();

    let stop_reason = match resp.status.as_str() {
        "completed" => Some(anthropic::StopReason::EndTurn),
        "incomplete" => Some(anthropic::StopReason::MaxTokens),
        "failed" => Some(anthropic::StopReason::EndTurn),
        other => {
            tracing::warn!(
                status = other,
                "unknown Responses API status, defaulting to end_turn"
            );
            Some(anthropic::StopReason::EndTurn)
        }
    };

    for item in &resp.output {
        extract_output_item(item, &mut content);
    }

    if content.is_empty() {
        content.push(anthropic::ContentBlock::Text {
            text: String::new(),
        });
    }

    let usage = resp
        .usage
        .as_ref()
        .map_or_else(anthropic::Usage::default, |u| {
            let cache_read_input_tokens =
                super::usage_map::extract_cached_tokens(u.input_token_details.as_ref());
            anthropic::Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_creation_input_tokens: None,
                cache_read_input_tokens,
            }
        });

    anthropic::MessageResponse {
        id: util::ids::generate_message_id(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content,
        model: original_model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage,
        created: None,
    }
}

/// Extract content blocks from a single output item (JSON value).
fn extract_output_item(item: &Value, content: &mut Vec<anthropic::ContentBlock>) {
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match item_type {
        "message" => {
            // OutputMessage: has content[] array
            if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                for part in parts {
                    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match part_type {
                        "output_text" => {
                            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                content.push(anthropic::ContentBlock::Text {
                                    text: text.to_string(),
                                });
                            }
                        }
                        "refusal" => {
                            if let Some(refusal) = part.get("refusal").and_then(|v| v.as_str()) {
                                content.push(anthropic::ContentBlock::Text {
                                    text: super::format_refusal(refusal),
                                });
                            }
                        }
                        _ => {
                            tracing::debug!(
                                part_type = part_type,
                                "unknown output content part type, skipped"
                            );
                        }
                    }
                }
            }
        }
        "function_call" => {
            // FunctionToolCall output item -> tool_use content block
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = item
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            if name.is_empty() {
                tracing::warn!("skipping function_call output with empty name");
                return;
            }

            let id = if call_id.is_empty() {
                let synthetic = util::ids::generate_tool_use_id();
                tracing::warn!(
                    name = name,
                    synthetic_id = synthetic,
                    "function_call had empty call_id; generated synthetic toolu_ ID"
                );
                synthetic
            } else {
                call_id.to_string()
            };

            let input = util::json::parse_tool_arguments(arguments);

            content.push(anthropic::ContentBlock::ToolUse {
                id,
                name: name.to_string(),
                input,
            });
        }
        _ => {
            tracing::debug!(item_type = item_type, "unknown output item type, skipped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn simple_request() -> anthropic::MessageCreateRequest {
        serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .unwrap()
    }

    #[test]
    fn basic_text_request() {
        let req = simple_request();
        let responses_req = anthropic_to_responses_request(&req);

        assert_eq!(responses_req.model, "claude-sonnet-4-6");
        assert_eq!(responses_req.max_output_tokens, Some(1024));
        assert!(responses_req.instructions.is_none());

        match &responses_req.input {
            ResponsesInput::Items(items) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0]["type"], "message");
                assert_eq!(items[0]["role"], "user");
                assert_eq!(items[0]["content"][0]["type"], "input_text");
                assert_eq!(items[0]["content"][0]["text"], "Hello");
            }
            _ => panic!("expected Items input"),
        }
    }

    #[test]
    fn system_prompt_to_instructions() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "system": "You are helpful",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        assert_eq!(responses_req.instructions, Some("You are helpful".into()));
    }

    #[test]
    fn multi_turn_conversation() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": "What is 2+2?"},
                {"role": "assistant", "content": "4"},
                {"role": "user", "content": "And 3+3?"}
            ]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        match &responses_req.input {
            ResponsesInput::Items(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0]["role"], "user");
                assert_eq!(items[1]["role"], "assistant");
                assert_eq!(items[2]["role"], "user");
            }
            _ => panic!("expected Items input"),
        }
    }

    #[test]
    fn tool_definitions_mapping() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a city",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }],
            "messages": [{"role": "user", "content": "Weather in NYC?"}]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        let tools = responses_req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Get weather for a city");
        assert!(tools[0]["parameters"]["properties"]["city"].is_object());
    }

    #[test]
    fn tool_use_in_assistant_message() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": "Weather?"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_123", "name": "get_weather", "input": {"city": "NYC"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_123", "content": "72F sunny"}
                ]}
            ]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        match &responses_req.input {
            ResponsesInput::Items(items) => {
                // user message, function_call item, function_call_output item
                assert_eq!(items.len(), 3);
                assert_eq!(items[0]["type"], "message");
                assert_eq!(items[1]["type"], "function_call");
                assert_eq!(items[1]["call_id"], "toolu_123");
                assert_eq!(items[1]["name"], "get_weather");
                assert_eq!(items[2]["type"], "function_call_output");
                assert_eq!(items[2]["call_id"], "toolu_123");
                assert_eq!(items[2]["output"], "72F sunny");
            }
            _ => panic!("expected Items input"),
        }
    }

    #[test]
    fn temperature_clamped() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "temperature": 1.5,
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        assert_eq!(responses_req.temperature, Some(1.0));
    }

    #[test]
    fn stop_sequences_truncated() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "stop_sequences": ["a", "b", "c", "d", "e"],
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        let stop = responses_req.extra.get("stop").unwrap().as_array().unwrap();
        assert_eq!(stop.len(), 4);
    }

    #[test]
    fn basic_text_response() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello!"}],
                "id": "msg_1",
                "status": "completed"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
            "status": "completed"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.model, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.role, anthropic::Role::Assistant);
        assert_eq!(
            anthropic_resp.stop_reason,
            Some(anthropic::StopReason::EndTurn)
        );
        assert_eq!(anthropic_resp.usage.input_tokens, 10);
        assert_eq!(anthropic_resp.usage.output_tokens, 5);

        match &anthropic_resp.content[0] {
            anthropic::ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn response_with_function_call() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_123",
                    "name": "get_weather",
                    "arguments": "{\"city\":\"NYC\"}",
                    "status": "completed"
                }
            ],
            "usage": {"input_tokens": 20, "output_tokens": 15, "total_tokens": 35},
            "status": "completed"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        match &anthropic_resp.content[0] {
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_123");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "NYC");
            }
            _ => panic!("expected tool_use block"),
        }
    }

    #[test]
    fn response_incomplete_status() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "partial..."}],
                "id": "msg_1",
                "status": "incomplete"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 100, "total_tokens": 110},
            "status": "incomplete"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        assert_eq!(
            anthropic_resp.stop_reason,
            Some(anthropic::StopReason::MaxTokens)
        );
    }

    #[test]
    fn response_no_usage() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hi"}],
                "id": "msg_1",
                "status": "completed"
            }],
            "status": "completed"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.usage.input_tokens, 0);
        assert_eq!(anthropic_resp.usage.output_tokens, 0);
    }

    #[test]
    fn response_mixed_text_and_tool_calls() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Let me check."}],
                    "id": "msg_1",
                    "status": "completed"
                },
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_456",
                    "name": "search",
                    "arguments": "{\"q\":\"weather\"}",
                    "status": "completed"
                }
            ],
            "usage": {"input_tokens": 15, "output_tokens": 20, "total_tokens": 35},
            "status": "completed"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.content.len(), 2);
        assert!(
            matches!(&anthropic_resp.content[0], anthropic::ContentBlock::Text { text } if text == "Let me check.")
        );
        assert!(
            matches!(&anthropic_resp.content[1], anthropic::ContentBlock::ToolUse { name, .. } if name == "search")
        );
    }

    #[test]
    fn empty_output_gets_empty_text() {
        let resp: ResponsesResponse = serde_json::from_value(json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [],
            "status": "completed"
        }))
        .unwrap();

        let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.content.len(), 1);
        assert!(
            matches!(&anthropic_resp.content[0], anthropic::ContentBlock::Text { text } if text.is_empty())
        );
    }

    #[test]
    fn tool_choice_mapping() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "tool_choice": {"type": "any"},
            "tools": [{"name": "f", "input_schema": {"type": "object"}}],
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        assert_eq!(responses_req.extra["tool_choice"], "required");
    }

    #[test]
    fn image_block_mapping() {
        let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 100,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}}
                ]
            }]
        }))
        .unwrap();

        let responses_req = anthropic_to_responses_request(&req);
        match &responses_req.input {
            ResponsesInput::Items(items) => {
                let content = items[0]["content"].as_array().unwrap();
                assert_eq!(content.len(), 2);
                assert_eq!(content[0]["type"], "input_text");
                assert_eq!(content[1]["type"], "input_image");
                assert_eq!(content[1]["image_url"], "https://example.com/img.png");
            }
            _ => panic!("expected Items input"),
        }
    }
}
