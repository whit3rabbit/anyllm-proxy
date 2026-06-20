use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{Map, Value};

use super::{ChatCompletionRequest, ChatRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallIdStrategy {
    Preserve,
    NineDigitSequential,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ToolCallNormalizationReport {
    pub duplicate_tool_call_ids: usize,
    pub missing_tool_call_ids: usize,
    pub remapped_tool_results: usize,
    pub orphan_tool_results: usize,
    pub normalized_tool_calls: usize,
}

impl ToolCallNormalizationReport {
    pub fn changed(self) -> bool {
        self.remapped_tool_results > 0
            || self.orphan_tool_results > 0
            || self.missing_tool_call_ids > 0
            || self.duplicate_tool_call_ids > 0
            || self.normalized_tool_calls > 0
    }
}

pub fn normalize_request_tool_call_ids(
    req: &mut ChatCompletionRequest,
    strategy: ToolCallIdStrategy,
) -> ToolCallNormalizationReport {
    let mut report = ToolCallNormalizationReport::default();
    let mut seen_raw: HashSet<String> = HashSet::new();

    if strategy == ToolCallIdStrategy::Preserve {
        for message in &req.messages {
            if !matches!(message.role, ChatRole::Assistant) {
                continue;
            }
            if let Some(calls) = &message.tool_calls {
                for call in calls {
                    if call.id.is_empty() {
                        report.missing_tool_call_ids += 1;
                    } else if !seen_raw.insert(call.id.clone()) {
                        report.duplicate_tool_call_ids += 1;
                    }
                }
            }
        }
        return report;
    }

    let mut seen_normalized: HashSet<String> = HashSet::new();
    let mut pending_tool_call_ids: HashMap<String, VecDeque<String>> = HashMap::new();
    let mut counter = 0usize;

    for message in &mut req.messages {
        if matches!(message.role, ChatRole::Assistant) {
            if let Some(calls) = &mut message.tool_calls {
                for call in calls {
                    let raw_id = (!call.id.is_empty()).then(|| call.id.clone());
                    match raw_id.as_deref() {
                        Some(id) => {
                            if !seen_raw.insert(id.to_string()) {
                                report.duplicate_tool_call_ids += 1;
                            }
                        }
                        None => report.missing_tool_call_ids += 1,
                    }

                    let normalized_id = next_numeric_call_id(&mut counter, &mut seen_normalized);
                    if let Some(raw_id) = raw_id {
                        pending_tool_call_ids
                            .entry(raw_id)
                            .or_default()
                            .push_back(normalized_id.clone());
                    }
                    call.id = normalized_id;
                }
            }
            continue;
        }

        if matches!(message.role, ChatRole::Tool) {
            let Some(raw_id) = message.tool_call_id.clone() else {
                continue;
            };
            let normalized_id = match pending_tool_call_ids
                .get_mut(&raw_id)
                .and_then(VecDeque::pop_front)
            {
                Some(paired) => {
                    if paired != raw_id {
                        report.remapped_tool_results += 1;
                    }
                    paired
                }
                None => {
                    report.orphan_tool_results += 1;
                    next_numeric_call_id(&mut counter, &mut seen_normalized)
                }
            };
            message.tool_call_id = Some(normalized_id);
        }
    }

    report
}

pub fn normalize_chat_completion_response_value(value: &mut Value) -> ToolCallNormalizationReport {
    normalize_choice_tool_calls(value, "message", true)
}

pub fn normalize_chat_completion_chunk_value(value: &mut Value) -> ToolCallNormalizationReport {
    normalize_choice_tool_calls(value, "delta", false)
}

fn next_numeric_call_id(counter: &mut usize, seen: &mut HashSet<String>) -> String {
    loop {
        let id = format!("{counter:09}");
        *counter += 1;
        if seen.insert(id.clone()) {
            return id;
        }
    }
}

fn normalize_choice_tool_calls(
    value: &mut Value,
    message_key: &str,
    default_missing_arguments: bool,
) -> ToolCallNormalizationReport {
    let mut report = ToolCallNormalizationReport::default();
    let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) else {
        return report;
    };

    for choice in choices {
        let Some(tool_calls) = choice
            .get_mut(message_key)
            .and_then(|message| message.get_mut("tool_calls"))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for tool_call in tool_calls {
            if normalize_tool_call_shape(tool_call, default_missing_arguments) {
                report.normalized_tool_calls += 1;
            }
        }
    }

    report
}

fn normalize_tool_call_shape(tool_call: &mut Value, default_missing_arguments: bool) -> bool {
    let Some(obj) = tool_call.as_object_mut() else {
        return false;
    };

    if obj.get("function").and_then(Value::as_object).is_some() {
        // Only stamp a synthetic `type` when normalizing a complete message
        // (response path, where `type` is a required field). On the streaming
        // chunk path (default_missing_arguments == false) a continuation delta
        // legitimately carries only `{index, function:{arguments}}` with no
        // `type`; injecting `type:"function"` on every fragment violates the
        // OpenAI streaming contract and can make a strict client split one tool
        // call into many.
        let mut changed = if default_missing_arguments {
            ensure_function_type(obj)
        } else {
            false
        };
        if let Some(function) = obj.get_mut("function").and_then(Value::as_object_mut) {
            changed |= normalize_function_arguments(function);
        }
        return changed;
    }

    let name = obj.get("name").cloned();
    let arguments = obj.get("arguments").cloned();
    if name.is_none() && arguments.is_none() {
        return false;
    }

    let mut function = Map::new();
    if let Some(name) = name {
        function.insert("name".to_string(), name);
    }
    if let Some(arguments) = arguments {
        function.insert("arguments".to_string(), normalized_arguments(arguments));
    } else if default_missing_arguments {
        function.insert("arguments".to_string(), Value::String("{}".to_string()));
    }
    ensure_function_type(obj);
    obj.insert("function".to_string(), Value::Object(function));
    true
}

fn ensure_function_type(tool_call: &mut Map<String, Value>) -> bool {
    if tool_call.contains_key("type") {
        return false;
    }
    tool_call.insert("type".to_string(), Value::String("function".to_string()));
    true
}

fn normalize_function_arguments(function: &mut Map<String, Value>) -> bool {
    let Some(arguments) = function.remove("arguments") else {
        return false;
    };
    let changed = !matches!(arguments, Value::String(_));
    let normalized = normalized_arguments(arguments);
    function.insert("arguments".to_string(), normalized);
    changed
}

fn normalized_arguments(arguments: Value) -> Value {
    match arguments {
        Value::String(_) => arguments,
        Value::Null => Value::String("{}".to_string()),
        other => Value::String(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::openai::{
        ChatCompletionChunk, ChatCompletionRequest, ChatContent, ChatMessage, ChatRole,
        FunctionCall, ToolCall,
    };

    fn request_with_tool_history() -> ChatCompletionRequest {
        serde_json::from_value(json!({
            "model": "mistral-large",
            "messages": [
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_000000000",
                            "type": "function",
                            "function": {"name": "run", "arguments": "{}"}
                        },
                        {
                            "id": "call_000000000",
                            "type": "function",
                            "function": {"name": "check", "arguments": "{}"}
                        },
                        {
                            "id": "",
                            "type": "function",
                            "function": {"name": "missing", "arguments": "{}"}
                        }
                    ]
                },
                {"role": "tool", "tool_call_id": "call_000000000", "content": "first"},
                {"role": "tool", "tool_call_id": "call_000000000", "content": "second"},
                {"role": "tool", "tool_call_id": "orphan", "content": "orphan"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn tool_normalization_rewrites_outbound_ids_and_preserves_pairing() {
        let mut req = request_with_tool_history();

        let report =
            normalize_request_tool_call_ids(&mut req, ToolCallIdStrategy::NineDigitSequential);

        let calls = req.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "000000000");
        assert_eq!(calls[1].id, "000000001");
        assert_eq!(calls[2].id, "000000002");
        assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("000000000"));
        assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("000000001"));
        assert_eq!(req.messages[3].tool_call_id.as_deref(), Some("000000003"));
        assert_eq!(report.duplicate_tool_call_ids, 1);
        assert_eq!(report.missing_tool_call_ids, 1);
        assert_eq!(report.remapped_tool_results, 2);
        assert_eq!(report.orphan_tool_results, 1);
    }

    #[test]
    fn tool_normalization_preserve_strategy_reports_without_rewriting() {
        let mut req = request_with_tool_history();

        let report = normalize_request_tool_call_ids(&mut req, ToolCallIdStrategy::Preserve);

        let calls = req.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "call_000000000");
        assert_eq!(calls[1].id, "call_000000000");
        assert_eq!(calls[2].id, "");
        assert_eq!(
            req.messages[1].tool_call_id.as_deref(),
            Some("call_000000000")
        );
        assert_eq!(report.duplicate_tool_call_ids, 1);
        assert_eq!(report.missing_tool_call_ids, 1);
        assert_eq!(report.remapped_tool_results, 0);
        assert_eq!(report.orphan_tool_results, 0);
    }

    #[test]
    fn tool_normalization_converts_top_level_response_tool_call_shape() {
        let mut value = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "model": "llama",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "name": "run",
                        "arguments": {"x": 1}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let report = normalize_chat_completion_response_value(&mut value);
        let response: crate::openai::ChatCompletionResponse =
            serde_json::from_value(value).unwrap();

        let tool_call = &response.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tool_call.call_type, "function");
        assert_eq!(tool_call.function.name, "run");
        assert_eq!(tool_call.function.arguments, "{\"x\":1}");
        assert_eq!(report.normalized_tool_calls, 1);
    }

    #[test]
    fn tool_normalization_converts_top_level_stream_delta_fragments() {
        let mut value = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "model": "llama",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "name": "run",
                        "arguments": "{\"x\""
                    }]
                }
            }]
        });

        let report = normalize_chat_completion_chunk_value(&mut value);
        let chunk: ChatCompletionChunk = serde_json::from_value(value).unwrap();

        let tool_call = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tool_call.call_type.as_deref(), Some("function"));
        assert_eq!(
            tool_call.function.as_ref().unwrap().name.as_deref(),
            Some("run")
        );
        assert_eq!(
            tool_call.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"x\"")
        );
        assert_eq!(report.normalized_tool_calls, 1);
    }

    #[test]
    fn tool_normalization_does_not_inject_type_into_continuation_deltas() {
        // A standard OpenAI continuation chunk carries only {index, function:{arguments}}
        // with no `id`/`type`/`name`. Normalization must NOT stamp `type:"function"`
        // onto it, or strict clients on the passthrough path may split one tool call.
        let mut value = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "model": "llama",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": ":1}"}
                    }]
                }
            }]
        });

        let report = normalize_chat_completion_chunk_value(&mut value);

        let tool_call = &value["choices"][0]["delta"]["tool_calls"][0];
        assert!(
            tool_call.get("type").is_none(),
            "continuation delta must not gain a synthetic type"
        );
        assert_eq!(tool_call["function"]["arguments"], json!(":1}"));
        assert_eq!(report.normalized_tool_calls, 0);
    }

    #[test]
    fn typed_request_normalization_handles_manual_structs() {
        let mut req = ChatCompletionRequest {
            model: "mistral-large".into(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: None,
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_000000000".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "run".into(),
                            arguments: "{}".into(),
                        },
                    }]),
                    tool_call_id: None,
                    refusal: None,
                    reasoning_content: None,
                },
                ChatMessage {
                    role: ChatRole::Tool,
                    content: Some(ChatContent::Text("ok".into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_000000000".into()),
                    refusal: None,
                    reasoning_content: None,
                },
            ],
            max_tokens: None,
            max_completion_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            stream: None,
            stream_options: None,
            presence_penalty: None,
            frequency_penalty: None,
            response_format: None,
            user: None,
            parallel_tool_calls: None,
            extra: serde_json::Map::new(),
        };

        normalize_request_tool_call_ids(&mut req, ToolCallIdStrategy::NineDigitSequential);

        let id = req.messages[0].tool_calls.as_ref().unwrap()[0].id.clone();
        assert_eq!(id, "000000000");
        assert_eq!(req.messages[1].tool_call_id.as_deref(), Some(id.as_str()));
    }
}
