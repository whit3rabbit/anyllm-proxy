use anyllm_translate::{anthropic, openai, TranslationWarnings};

#[derive(Debug, Default)]
pub(super) struct AnthropicChatExtensions {
    pub(super) raw_tools: Vec<serde_json::Value>,
    pub(super) beta_headers: Vec<&'static str>,
}

pub(super) fn merge_anthropic_beta_headers(
    headers: &mut Vec<(String, String)>,
    betas: &[&'static str],
) {
    if betas.is_empty() {
        return;
    }
    let mut values = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for beta in betas {
        if !values.iter().any(|value| value == beta) {
            values.push((*beta).to_string());
        }
    }

    let merged = values.join(",");
    if let Some((_, value)) = headers
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
    {
        *value = merged;
    } else {
        headers.push(("anthropic-beta".to_string(), merged));
    }
}

pub(super) fn serialize_anthropic_upstream_request(
    req: &anthropic::MessageCreateRequest,
    raw_tools: &[serde_json::Value],
) -> Result<bytes::Bytes, serde_json::Error> {
    let mut value = serde_json::to_value(req)?;
    if !raw_tools.is_empty() {
        let obj = value.as_object_mut().expect("request serializes to object");
        let tools = obj
            .entry("tools".to_string())
            .or_insert_with(|| serde_json::json!([]));
        if !tools.is_array() {
            *tools = serde_json::json!([]);
        }
        tools
            .as_array_mut()
            .expect("tools coerced to array")
            .extend(raw_tools.iter().cloned());
    }
    serde_json::to_vec(&value).map(bytes::Bytes::from)
}

pub(super) fn apply_anthropic_chat_extensions(
    openai_req: &openai::ChatCompletionRequest,
    anthropic_req: &mut anthropic::MessageCreateRequest,
    warnings: &mut TranslationWarnings,
    caller_omitted_max_tokens: bool,
) -> Result<AnthropicChatExtensions, String> {
    let mut extensions = AnthropicChatExtensions::default();
    let mut output_config = anthropic_req
        .extra
        .remove("output_config")
        .unwrap_or_else(|| serde_json::json!({}));
    if !output_config.is_object() {
        output_config = serde_json::json!({});
    }

    apply_reasoning_effort(
        openai_req,
        anthropic_req,
        &mut output_config,
        caller_omitted_max_tokens,
    )?;

    if let Some(response_format) = &openai_req.response_format {
        if response_format.format_type == "json_schema" {
            if let Some(json_schema) = &response_format.json_schema {
                let schema = json_schema
                    .get("schema")
                    .cloned()
                    .unwrap_or_else(|| json_schema.clone());
                output_config["format"] = serde_json::json!({
                    "type": "json_schema",
                    "schema": sanitize_anthropic_output_schema(schema),
                });
                warnings.remove("response_format");
            }
        }
    }

    if let Some(web_search_options) = openai_req.extra.get("web_search_options") {
        if let Some(tool) = map_web_search_options(web_search_options) {
            extensions.raw_tools.push(tool);
        }
    }
    if let Some(cache_control) = openai_req.extra.get("cache_control") {
        if cache_control.is_object() {
            anthropic_req
                .extra
                .insert("cache_control".to_string(), cache_control.clone());
        }
    }
    if let Some(speed) = openai_req.extra.get("speed").and_then(|v| v.as_str()) {
        anthropic_req.extra.insert(
            "speed".to_string(),
            serde_json::Value::String(speed.to_string()),
        );
        if speed == "fast" {
            extensions.beta_headers.push("fast-mode-2026-02-01");
        }
    }
    if let Some(context_management) = openai_req.extra.get("context_management") {
        if let Some(normalized) = normalize_context_management(context_management) {
            add_context_management_beta_headers(&normalized, &mut extensions.beta_headers);
            anthropic_req
                .extra
                .insert("context_management".to_string(), normalized);
        }
    }

    if output_config
        .as_object()
        .map(|o| !o.is_empty())
        .unwrap_or(false)
    {
        anthropic_req
            .extra
            .insert("output_config".to_string(), output_config);
    }
    Ok(extensions)
}

fn apply_reasoning_effort(
    openai_req: &openai::ChatCompletionRequest,
    anthropic_req: &mut anthropic::MessageCreateRequest,
    output_config: &mut serde_json::Value,
    caller_omitted_max_tokens: bool,
) -> Result<(), String> {
    let Some(effort) = reasoning_effort_value(openai_req) else {
        return Ok(());
    };
    if effort == "none" {
        anthropic_req.thinking = None;
        return Ok(());
    }
    if should_omit_thinking_for_tool_continuation(anthropic_req) {
        return Ok(());
    }

    let output_effort = match effort {
        "minimal" | "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" => "xhigh",
        "max" => "max",
        other => {
            return Err(format!(
                "Invalid reasoning_effort: {other}. Expected one of minimal, low, medium, high, xhigh, max, none."
            ));
        }
    };

    if anyllm_providers::model_supports_anthropic_adaptive_thinking(
        "anthropic",
        &anthropic_req.model,
    ) {
        if !anyllm_providers::model_supports_anthropic_reasoning_effort(
            "anthropic",
            &anthropic_req.model,
            effort,
        ) {
            return Err(format!(
                "model {} does not support reasoning_effort {effort}",
                anthropic_req.model
            ));
        }
        anthropic_req.thinking = Some(anthropic::ThinkingConfig::Adaptive {
            extra: serde_json::Map::new(),
        });
        output_config["effort"] = serde_json::Value::String(output_effort.to_string());
        return Ok(());
    }

    let budget = match effort {
        "minimal" | "low" => 1024,
        "medium" => 2048,
        "high" => 4096,
        "xhigh" => 8192,
        "max" => 16384,
        other => {
            return Err(format!(
                "Invalid reasoning_effort: {other}. Expected one of minimal, low, medium, high, xhigh, max, none."
            ));
        }
    };

    if caller_omitted_max_tokens {
        anthropic_req.max_tokens = budget + 4096;
    } else if anthropic_req.max_tokens <= budget {
        return Err(format!(
            "max_tokens must be greater than Anthropic thinking budget_tokens ({budget})"
        ));
    }
    anthropic_req.thinking = Some(anthropic::ThinkingConfig::Enabled {
        budget_tokens: budget,
    });
    Ok(())
}

fn reasoning_effort_value(req: &openai::ChatCompletionRequest) -> Option<&str> {
    let value = req.extra.get("reasoning_effort")?;
    if let Some(effort) = value.as_str() {
        return Some(effort);
    }
    value
        .as_object()
        .and_then(|obj| obj.get("effort"))
        .and_then(|v| v.as_str())
}

fn should_omit_thinking_for_tool_continuation(req: &anthropic::MessageCreateRequest) -> bool {
    let latest_assistant_has_tool_use = req
        .messages
        .iter()
        .rev()
        .find(|message| message.role == anthropic::Role::Assistant)
        .map(|message| {
            let anthropic::Content::Blocks(blocks) = &message.content else {
                return false;
            };
            blocks
                .iter()
                .any(|block| matches!(block, anthropic::ContentBlock::ToolUse { .. }))
        })
        .unwrap_or(false);
    if !latest_assistant_has_tool_use {
        return false;
    }

    !req.messages.iter().any(|message| {
        let anthropic::Content::Blocks(blocks) = &message.content else {
            return false;
        };
        blocks.iter().any(|block| match block {
            anthropic::ContentBlock::Thinking {
                signature: Some(signature),
                ..
            } => !signature.is_empty(),
            anthropic::ContentBlock::RedactedThinking { .. } => true,
            _ => false,
        })
    })
}

fn sanitize_anthropic_output_schema(schema: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut obj) = schema else {
        return schema;
    };

    const UNSUPPORTED: &[(&str, &str)] = &[
        ("minItems", "minimum number of items"),
        ("maxItems", "maximum number of items"),
        ("minimum", "minimum value"),
        ("maximum", "maximum value"),
        ("exclusiveMinimum", "exclusive minimum value"),
        ("exclusiveMaximum", "exclusive maximum value"),
        ("minLength", "minimum length"),
        ("maxLength", "maximum length"),
    ];

    let mut notes = Vec::new();
    for (field, label) in UNSUPPORTED {
        if let Some(value) = obj.remove(*field) {
            notes.push(format!("{label}: {value}"));
        }
    }
    if !notes.is_empty() {
        let note = format!("Note: {}.", notes.join(", "));
        let description = obj
            .remove("description")
            .and_then(|v| v.as_str().map(ToString::to_string))
            .map(|existing| format!("{existing} {note}"))
            .unwrap_or(note);
        obj.insert(
            "description".to_string(),
            serde_json::Value::String(description),
        );
    }

    if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
        for value in properties.values_mut() {
            *value = sanitize_anthropic_output_schema(value.clone());
        }
    }
    if let Some(items) = obj.get("items").cloned() {
        obj.insert("items".to_string(), sanitize_anthropic_output_schema(items));
    }
    for key in ["$defs", "definitions"] {
        if let Some(defs) = obj.get_mut(key).and_then(|v| v.as_object_mut()) {
            for value in defs.values_mut() {
                *value = sanitize_anthropic_output_schema(value.clone());
            }
        }
    }
    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(items) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            for value in items.iter_mut() {
                *value = sanitize_anthropic_output_schema(value.clone());
            }
        }
    }
    if obj.get("type").and_then(|v| v.as_str()) == Some("object") {
        obj.entry("additionalProperties".to_string())
            .or_insert(serde_json::Value::Bool(false));
    }

    serde_json::Value::Object(obj)
}

fn map_web_search_options(value: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = value.as_object()?;
    let mut tool = serde_json::json!({
        "type": "web_search_20250305",
        "name": "web_search"
    });

    if let Some(max_uses) = obj
        .get("search_context_size")
        .and_then(|v| v.as_str())
        .and_then(|size| match size {
            "low" => Some(1),
            "medium" => Some(5),
            "high" => Some(10),
            _ => None,
        })
    {
        tool["max_uses"] = serde_json::json!(max_uses);
    }

    if let Some(approximate) = obj
        .get("user_location")
        .and_then(|v| v.get("approximate"))
        .and_then(|v| v.as_object())
    {
        let mut location = serde_json::Map::new();
        location.insert(
            "type".to_string(),
            serde_json::Value::String("approximate".to_string()),
        );
        for (key, value) in approximate {
            if key != "type" {
                location.insert(key.clone(), value.clone());
            }
        }
        tool["user_location"] = serde_json::Value::Object(location);
    }

    Some(tool)
}

fn normalize_context_management(value: &serde_json::Value) -> Option<serde_json::Value> {
    if value.get("edits").is_some() {
        return Some(value.clone());
    }
    let entries = value.as_array()?;
    let mut edits = Vec::new();
    for entry in entries {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        if obj.get("type").and_then(|v| v.as_str()) == Some("compaction") {
            let mut edit = serde_json::Map::new();
            edit.insert(
                "type".to_string(),
                serde_json::Value::String("compact_20260112".to_string()),
            );
            if let Some(threshold) = obj.get("compact_threshold").and_then(|v| v.as_u64()) {
                edit.insert(
                    "trigger".to_string(),
                    serde_json::json!({"type": "input_tokens", "value": threshold}),
                );
            }
            for (key, value) in obj {
                if key != "type" && key != "compact_threshold" {
                    edit.insert(key.clone(), value.clone());
                }
            }
            edits.push(serde_json::Value::Object(edit));
        } else {
            edits.push(serde_json::Value::Object(obj.clone()));
        }
    }
    if edits.is_empty() {
        None
    } else {
        Some(serde_json::json!({ "edits": edits }))
    }
}

fn add_context_management_beta_headers(
    value: &serde_json::Value,
    beta_headers: &mut Vec<&'static str>,
) {
    let Some(edits) = value.get("edits").and_then(|v| v.as_array()) else {
        return;
    };
    let mut has_compact = false;
    let mut has_other = false;
    for edit in edits {
        match edit.get("type").and_then(|v| v.as_str()) {
            Some("compact_20260112") | Some("compaction") => has_compact = true,
            Some(_) => has_other = true,
            None => {}
        }
    }
    if has_compact {
        beta_headers.push("compact-2026-01-12");
    }
    if has_other {
        beta_headers.push("context-management-2025-06-27");
    }
}

#[cfg(test)]
mod tests;
