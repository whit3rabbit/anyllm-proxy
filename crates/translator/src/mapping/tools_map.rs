//! Tool definition and tool_choice mapping between Anthropic and OpenAI APIs.
//!
//! Covers: tool definitions, tool_choice, strict-mode schema normalization,
//! Gemini schema sanitization, and tool result/call content block conversion.

use crate::anthropic;
use crate::openai;

/// Convert Anthropic tool definitions to OpenAI tool definitions.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
pub fn anthropic_tools_to_openai(tools: &[anthropic::Tool]) -> Vec<openai::ChatTool> {
    tools
        .iter()
        .map(|t| openai::ChatTool {
            tool_type: "function".to_string(),
            function: openai::FunctionDef {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: Some(t.input_schema.clone()),
                // Compat spec: "Ignored". Anthropic has no equivalent.
                // See: https://docs.anthropic.com/en/api/openai-sdk#tools--functions-fields
                strict: None,
            },
        })
        .collect()
}

/// Convert OpenAI tool definitions back to Anthropic tool definitions.
/// When parameters is None, defaults to `{"type": "object"}` since Anthropic
/// requires input_schema to be present.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn openai_tools_to_anthropic(tools: &[openai::ChatTool]) -> Vec<anthropic::Tool> {
    tools
        .iter()
        .map(|t| anthropic::Tool {
            name: t.function.name.clone(),
            description: t.function.description.clone(),
            input_schema: coerce_anthropic_input_schema(
                t.function
                    .parameters
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({})),
            ),
        })
        .collect()
}

/// Anthropic tool schemas must be object schemas. Preserve existing object
/// schema detail, but coerce absent or non-object parameters to a no-arg object.
pub fn coerce_anthropic_input_schema(schema: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut obj) = schema else {
        return serde_json::json!({"type": "object", "properties": {}});
    };

    if obj.get("type").and_then(|v| v.as_str()) != Some("object") {
        obj.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
    }
    obj.entry("properties".to_string())
        .or_insert_with(|| serde_json::json!({}));
    serde_json::Value::Object(obj)
}

/// JSON Schema keys that Gemini's function-calling API rejects.
/// Gemini supports only the OpenAPI 3.0 subset of JSON Schema.
const GEMINI_DISALLOWED_SCHEMA_KEYS: &[&str] = &[
    "$schema",
    "anyOf",
    "oneOf",
    "allOf",
    "not",
    "default",
    "const",
    "$defs",
    "definitions",
    "additionalProperties",
    "$ref",
    "if",
    "then",
    "else",
];

/// Recursively strip JSON Schema fields that Gemini rejects.
/// Applied to tool `parameters` when the backend is Gemini or Vertex.
pub fn sanitize_schema_for_gemini(schema: serde_json::Value) -> serde_json::Value {
    match schema {
        serde_json::Value::Object(mut map) => {
            for key in GEMINI_DISALLOWED_SCHEMA_KEYS {
                map.remove(*key);
            }
            let sanitized: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, sanitize_schema_for_gemini(v)))
                .collect();
            serde_json::Value::Object(sanitized)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sanitize_schema_for_gemini).collect())
        }
        other => other,
    }
}

/// Convert Anthropic tool_choice to OpenAI tool_choice.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
pub fn anthropic_tool_choice_to_openai(tc: &anthropic::ToolChoice) -> openai::ChatToolChoice {
    match tc {
        anthropic::ToolChoice::Auto { .. } => openai::ChatToolChoice::Simple("auto".to_string()),
        // Any = "model must call at least one tool". OpenAI's "required"
        // is the closest: it forces a tool call when tools are defined.
        anthropic::ToolChoice::Any { .. } => openai::ChatToolChoice::Simple("required".to_string()),
        anthropic::ToolChoice::None => openai::ChatToolChoice::Simple("none".to_string()),
        anthropic::ToolChoice::Tool { name } => {
            openai::ChatToolChoice::Named(openai::chat_completions::NamedToolChoice {
                choice_type: "function".to_string(),
                function: openai::chat_completions::NamedFunction { name: name.clone() },
            })
        }
    }
}

/// Convert OpenAI tool_choice to Anthropic tool_choice.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn openai_tool_choice_to_anthropic(tc: &openai::ChatToolChoice) -> anthropic::ToolChoice {
    match tc {
        openai::ChatToolChoice::Simple(s) => match s.as_str() {
            "none" => anthropic::ToolChoice::None,
            "required" => anthropic::ToolChoice::Any {
                disable_parallel_tool_use: None,
            },
            // Default unknown values to Auto for forward compatibility;
            // rejecting would break when OpenAI adds new tool_choice variants.
            _ => anthropic::ToolChoice::Auto {
                disable_parallel_tool_use: None,
            },
        },
        openai::ChatToolChoice::Named(named) => anthropic::ToolChoice::Tool {
            name: named.function.name.clone(),
        },
    }
}

/// Normalize a JSON Schema for OpenAI strict mode.
///
/// OpenAI strict mode requires:
/// - All properties of object schemas listed in `required`.
/// - `additionalProperties: false` on all object schemas (including nested objects
///   inside `anyOf`, `oneOf`, `allOf`, `items`, `$defs`, and `definitions`).
///
/// Applied recursively through all schema combinators, not just direct properties.
pub fn normalize_schema_for_strict(schema: serde_json::Value) -> serde_json::Value {
    let mut schema = schema;

    let Some(obj) = schema.as_object_mut() else {
        return schema;
    };

    // Apply object constraints only when this schema is explicitly type:object.
    if obj.get("type").and_then(|t| t.as_str()) == Some("object") {
        obj.insert(
            "additionalProperties".to_string(),
            serde_json::Value::Bool(false),
        );

        let prop_keys: Vec<String> = obj
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();

        if !prop_keys.is_empty() {
            let existing: std::collections::HashSet<String> = obj
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let mut merged: Vec<String> = prop_keys.into_iter().chain(existing).collect();
            merged.sort();
            merged.dedup();

            obj.insert(
                "required".to_string(),
                serde_json::Value::Array(
                    merged.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }
    }

    // Recurse into all properties (regardless of their type — they may be combinators).
    if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for prop_val in props.values_mut() {
            *prop_val = normalize_schema_for_strict(prop_val.clone());
        }
    }

    // Recurse into anyOf / oneOf / allOf schema combinators.
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            for item in arr.iter_mut() {
                *item = normalize_schema_for_strict(item.clone());
            }
        }
    }

    // Recurse into array item schemas.
    if let Some(items) = obj.get("items").cloned() {
        let normalized = normalize_schema_for_strict(items);
        obj.insert("items".to_string(), normalized);
    }

    // Recurse into $defs / definitions.
    for key in ["$defs", "definitions"] {
        if let Some(defs) = obj.get_mut(key).and_then(|v| v.as_object_mut()) {
            for def_val in defs.values_mut() {
                *def_val = normalize_schema_for_strict(def_val.clone());
            }
        }
    }

    schema
}

/// Apply strict mode to the single tool that is being forced via tool_choice.
///
/// Finds the tool whose function name matches `forced_name`, sets `strict: true`
/// on its function object, and normalizes its parameter schema.
///
/// All other tools are left unchanged.
pub fn apply_strict_to_forced_tool(tools: &mut [serde_json::Value], forced_name: &str) {
    for tool in tools.iter_mut() {
        let Some(function) = tool.get_mut("function") else {
            continue;
        };
        let name_matches = function.get("name").and_then(|n| n.as_str()) == Some(forced_name);

        if name_matches {
            if let Some(obj) = function.as_object_mut() {
                obj.insert("strict".to_string(), serde_json::Value::Bool(true));

                // OpenAI strict mode requires parameters to be a valid object schema.
                // If absent or null (e.g., a no-argument tool), coerce to the minimal
                // valid schema; an absent/null parameters field with strict:true causes a 400.
                let params = obj
                    .get("parameters")
                    .cloned()
                    .filter(|v| !v.is_null())
                    .unwrap_or_else(|| {
                        serde_json::json!({
                            "type": "object",
                            "properties": {},
                            "required": [],
                            "additionalProperties": false
                        })
                    });
                obj.insert(
                    "parameters".to_string(),
                    normalize_schema_for_strict(params),
                );
            }
            // Tool names are unique; stop after the first match.
            break;
        }
    }
}

#[cfg(test)]
mod tests;
