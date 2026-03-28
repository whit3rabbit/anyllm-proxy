// Tool definition and tool_choice mapping

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
            input_schema: t
                .function
                .parameters
                .clone()
                .unwrap_or_else(|| serde_json::json!({"type": "object"})),
        })
        .collect()
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
/// - `additionalProperties: false` on all object schemas.
///
/// Applied recursively to nested object schemas.
/// Non-object schemas are returned unchanged.
pub fn normalize_schema_for_strict(mut schema: serde_json::Value) -> serde_json::Value {
    if schema.get("type").and_then(|t| t.as_str()) != Some("object") {
        return schema;
    }

    let Some(obj) = schema.as_object_mut() else {
        return schema;
    };

    // Set additionalProperties: false.
    obj.insert(
        "additionalProperties".to_string(),
        serde_json::Value::Bool(false),
    );

    // Collect all property keys.
    let prop_keys: Vec<String> = obj
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();

    if !prop_keys.is_empty() {
        // Merge with any existing required array.
        let existing: std::collections::HashSet<String> = obj
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let mut merged: Vec<String> = prop_keys
            .into_iter()
            .chain(existing)
            .collect();
        merged.sort();
        merged.dedup();

        obj.insert(
            "required".to_string(),
            serde_json::Value::Array(
                merged.into_iter().map(serde_json::Value::String).collect(),
            ),
        );

        // Recurse into nested object properties.
        if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
            for prop_val in props.values_mut() {
                if prop_val.get("type").and_then(|t| t.as_str()) == Some("object") {
                    *prop_val = normalize_schema_for_strict(prop_val.clone());
                }
            }
        }
    }

    serde_json::Value::Object(obj.clone())
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
        let name_matches = function
            .get("name")
            .and_then(|n| n.as_str())
            == Some(forced_name);

        if name_matches {
            if let Some(obj) = function.as_object_mut() {
                obj.insert("strict".to_string(), serde_json::Value::Bool(true));

                // Normalize parameter schema in place.
                if let Some(params) = obj.get("parameters").cloned() {
                    obj.insert(
                        "parameters".to_string(),
                        normalize_schema_for_strict(params),
                    );
                }
            }
            // Tool names are unique; stop after the first match.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn sample_anthropic_tool() -> anthropic::Tool {
        anthropic::Tool {
            name: "get_weather".into(),
            description: Some("Get weather for a location".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        }
    }

    fn sample_openai_tool() -> openai::ChatTool {
        openai::ChatTool {
            tool_type: "function".into(),
            function: openai::FunctionDef {
                name: "get_weather".into(),
                description: Some("Get weather for a location".into()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                })),
                strict: None,
            },
        }
    }

    // --- Tool definition conversion ---

    #[test]
    fn anthropic_to_openai_tool() {
        let tools = anthropic_tools_to_openai(&[sample_anthropic_tool()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "function");
        assert_eq!(tools[0].function.name, "get_weather");
        assert_eq!(
            tools[0].function.description.as_deref(),
            Some("Get weather for a location")
        );
        assert_eq!(
            tools[0].function.parameters,
            Some(sample_anthropic_tool().input_schema)
        );
    }

    #[test]
    fn openai_to_anthropic_tool() {
        let tools = openai_tools_to_anthropic(&[sample_openai_tool()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
        assert_eq!(
            tools[0].description.as_deref(),
            Some("Get weather for a location")
        );
        assert_eq!(tools[0].input_schema, sample_anthropic_tool().input_schema);
    }

    #[test]
    fn empty_tools_list() {
        assert!(anthropic_tools_to_openai(&[]).is_empty());
        assert!(openai_tools_to_anthropic(&[]).is_empty());
    }

    #[test]
    fn tool_without_description() {
        let tool = anthropic::Tool {
            name: "no_desc".into(),
            description: None,
            input_schema: json!({"type": "object"}),
        };
        let openai = anthropic_tools_to_openai(&[tool]);
        assert!(openai[0].function.description.is_none());

        // And back
        let anthropic = openai_tools_to_anthropic(&openai);
        assert!(anthropic[0].description.is_none());
    }

    #[test]
    fn openai_tool_without_parameters_defaults_to_object() {
        let tool = openai::ChatTool {
            tool_type: "function".into(),
            function: openai::FunctionDef {
                name: "simple".into(),
                description: None,
                parameters: None,
                strict: None,
            },
        };
        let anthropic = openai_tools_to_anthropic(&[tool]);
        assert_eq!(anthropic[0].input_schema, json!({"type": "object"}));
    }

    #[test]
    fn multiple_tools_preserved() {
        let tools = vec![
            anthropic::Tool {
                name: "tool_a".into(),
                description: Some("A".into()),
                input_schema: json!({"type": "object"}),
            },
            anthropic::Tool {
                name: "tool_b".into(),
                description: Some("B".into()),
                input_schema: json!({"type": "object", "properties": {"x": {"type": "number"}}}),
            },
        ];
        let openai = anthropic_tools_to_openai(&tools);
        assert_eq!(openai.len(), 2);
        assert_eq!(openai[0].function.name, "tool_a");
        assert_eq!(openai[1].function.name, "tool_b");

        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "tool_a");
        assert_eq!(back[1].name, "tool_b");
        assert_eq!(back[1].input_schema, tools[1].input_schema);
    }

    // --- Tool choice mapping ---

    #[test]
    fn tool_choice_auto() {
        let openai = anthropic_tool_choice_to_openai(&anthropic::ToolChoice::Auto {
            disable_parallel_tool_use: None,
        });
        assert!(matches!(openai, openai::ChatToolChoice::Simple(ref s) if s == "auto"));

        let back = openai_tool_choice_to_anthropic(&openai);
        assert!(matches!(back, anthropic::ToolChoice::Auto { .. }));
    }

    #[test]
    fn tool_choice_any_to_required() {
        let openai = anthropic_tool_choice_to_openai(&anthropic::ToolChoice::Any {
            disable_parallel_tool_use: None,
        });
        assert!(matches!(openai, openai::ChatToolChoice::Simple(ref s) if s == "required"));

        let back = openai_tool_choice_to_anthropic(&openai);
        assert!(matches!(back, anthropic::ToolChoice::Any { .. }));
    }

    #[test]
    fn tool_choice_none() {
        let openai = anthropic_tool_choice_to_openai(&anthropic::ToolChoice::None);
        assert!(matches!(openai, openai::ChatToolChoice::Simple(ref s) if s == "none"));

        let back = openai_tool_choice_to_anthropic(&openai);
        assert!(matches!(back, anthropic::ToolChoice::None));
    }

    #[test]
    fn tool_choice_specific_tool() {
        let tc = anthropic::ToolChoice::Tool {
            name: "get_weather".into(),
        };
        let openai = anthropic_tool_choice_to_openai(&tc);
        match &openai {
            openai::ChatToolChoice::Named(named) => {
                assert_eq!(named.choice_type, "function");
                assert_eq!(named.function.name, "get_weather");
            }
            _ => panic!("expected Named tool choice"),
        }

        let back = openai_tool_choice_to_anthropic(&openai);
        match back {
            anthropic::ToolChoice::Tool { name } => assert_eq!(name, "get_weather"),
            other => panic!("expected ToolChoice::Tool, got {:?}", other),
        }
    }

    #[test]
    fn openai_unknown_simple_choice_defaults_to_auto() {
        // Any unrecognized simple string should map to Auto
        let tc = openai::ChatToolChoice::Simple("something_else".into());
        assert!(matches!(
            openai_tool_choice_to_anthropic(&tc),
            anthropic::ToolChoice::Auto { .. }
        ));
    }

    #[test]
    fn disable_parallel_tool_use_roundtrips_via_serde() {
        // Ensure the field survives JSON deserialization
        let json = serde_json::json!({"type": "auto", "disable_parallel_tool_use": true});
        let tc: anthropic::ToolChoice = serde_json::from_value(json).unwrap();
        match tc {
            anthropic::ToolChoice::Auto {
                disable_parallel_tool_use: Some(true),
            } => {}
            other => panic!(
                "expected Auto with disable_parallel_tool_use=true, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn auto_without_disable_parallel_omits_field_in_json() {
        let tc = anthropic::ToolChoice::Auto {
            disable_parallel_tool_use: None,
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json, serde_json::json!({"type": "auto"}));
    }

    // --- Claude Code tool schema round-trips ---

    #[test]
    fn claude_code_read_tool_roundtrip() {
        let tool = anthropic::Tool {
            name: "Read".into(),
            description: Some("Reads a file from the local filesystem.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "The absolute path to the file to read"},
                    "offset": {"type": "number", "description": "The line number to start reading from"},
                    "limit": {"type": "number", "description": "The number of lines to read"}
                },
                "required": ["file_path"]
            }),
        };
        let openai = anthropic_tools_to_openai(&[tool.clone()]);
        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back[0].name, tool.name);
        assert_eq!(back[0].description, tool.description);
        assert_eq!(back[0].input_schema, tool.input_schema);
    }

    #[test]
    fn claude_code_bash_tool_roundtrip() {
        let tool = anthropic::Tool {
            name: "Bash".into(),
            description: Some("Executes a given bash command and returns its output.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The command to execute"},
                    "description": {"type": "string", "description": "Description of the command"},
                    "timeout": {"type": "number", "description": "Optional timeout in milliseconds"}
                },
                "required": ["command"]
            }),
        };
        let openai = anthropic_tools_to_openai(&[tool.clone()]);
        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back[0].name, tool.name);
        assert_eq!(back[0].input_schema, tool.input_schema);
    }

    #[test]
    fn claude_code_edit_tool_roundtrip() {
        let tool = anthropic::Tool {
            name: "Edit".into(),
            description: Some("Performs exact string replacements in files.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "old_string": {"type": "string"},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean", "default": false}
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        };
        let openai = anthropic_tools_to_openai(&[tool.clone()]);
        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back[0].name, tool.name);
        assert_eq!(back[0].input_schema, tool.input_schema);
    }

    #[test]
    fn claude_code_grep_tool_with_enum_roundtrip() {
        // Grep has an enum field (output_mode) which must survive translation
        let tool = anthropic::Tool {
            name: "Grep".into(),
            description: Some("A powerful search tool built on ripgrep.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string"},
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files_with_matches", "count"]
                    }
                },
                "required": ["pattern"]
            }),
        };
        let openai = anthropic_tools_to_openai(&[tool.clone()]);
        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back[0].input_schema, tool.input_schema);
    }

    #[test]
    fn claude_code_all_six_tools_preserved() {
        // All 6 core Claude Code tools survive batch translation
        let tools: Vec<anthropic::Tool> = ["Read", "Write", "Edit", "Bash", "Glob", "Grep"]
            .iter()
            .map(|name| anthropic::Tool {
                name: (*name).to_string(),
                description: Some(format!("{} tool", name)),
                input_schema: json!({"type": "object"}),
            })
            .collect();
        let openai = anthropic_tools_to_openai(&tools);
        assert_eq!(openai.len(), 6);
        let back = openai_tools_to_anthropic(&openai);
        assert_eq!(back.len(), 6);
        for (orig, rt) in tools.iter().zip(back.iter()) {
            assert_eq!(orig.name, rt.name);
        }
    }

    // --- Gemini schema sanitization ---

    #[test]
    fn sanitize_strips_disallowed_top_level_fields() {
        let schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema",
            "type": "object",
            "default": {},
            "additionalProperties": false,
            "$defs": {"myType": {"type": "string"}}
        });
        let result = sanitize_schema_for_gemini(schema);
        assert!(result.get("$schema").is_none());
        assert!(result.get("default").is_none());
        assert!(result.get("$defs").is_none());
        assert!(result.get("additionalProperties").is_none());
        assert_eq!(result["type"], "object");
    }

    #[test]
    fn sanitize_strips_disallowed_nested_fields() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "default": "unnamed", "const": "fixed"},
                "count": {"anyOf": [{"type": "integer"}, {"type": "null"}]}
            }
        });
        let result = sanitize_schema_for_gemini(schema);
        let name_prop = &result["properties"]["name"];
        assert!(name_prop.get("default").is_none());
        assert!(name_prop.get("const").is_none());
        assert_eq!(name_prop["type"], "string");
        let count_prop = &result["properties"]["count"];
        assert!(count_prop.get("anyOf").is_none());
    }

    #[test]
    fn sanitize_leaves_valid_schema_unchanged() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "city": {"type": "string"},
                "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
            },
            "required": ["city"]
        });
        let result = sanitize_schema_for_gemini(schema.clone());
        assert_eq!(result, schema);
    }
}

#[cfg(test)]
mod strict_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_adds_required_and_disables_additional_props() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let normalized = normalize_schema_for_strict(schema);
        let required = normalized["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "name"), "name should be required");
        assert!(required.iter().any(|v| v == "age"), "age should be required");
        assert_eq!(normalized["additionalProperties"], json!(false));
    }

    #[test]
    fn normalize_nested_object_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "object",
                    "properties": {
                        "street": {"type": "string"}
                    }
                }
            }
        });
        let normalized = normalize_schema_for_strict(schema);
        // Nested object must also have required and additionalProperties.
        let nested = &normalized["properties"]["address"];
        assert_eq!(nested["additionalProperties"], json!(false));
        let nested_required = nested["required"].as_array().unwrap();
        assert!(nested_required.iter().any(|v| v == "street"));
    }

    #[test]
    fn normalize_preserves_existing_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "x": {"type": "string"},
                "y": {"type": "string"}
            },
            "required": ["x"]
        });
        // Should merge existing required with all properties.
        let normalized = normalize_schema_for_strict(schema);
        let required = normalized["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "x"));
        assert!(required.iter().any(|v| v == "y"));
    }

    #[test]
    fn normalize_non_object_schema_unchanged() {
        let schema = json!({"type": "string"});
        let normalized = normalize_schema_for_strict(schema.clone());
        assert_eq!(normalized, schema);
    }

    #[test]
    fn apply_strict_to_forced_tool_sets_strict_flag() {
        let mut tools: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "send_email",
                    "description": "Send email",
                    "parameters": {
                        "type": "object",
                        "properties": {"to": {"type": "string"}, "body": {"type": "string"}}
                    }
                }
            }),
        ];

        apply_strict_to_forced_tool(&mut tools, "send_email");

        // Only send_email should have strict: true.
        let send_email = &tools[1]["function"];
        assert_eq!(send_email["strict"], serde_json::json!(true));
        assert_eq!(
            send_email["parameters"]["additionalProperties"],
            serde_json::json!(false)
        );

        // get_weather should be unchanged (no strict flag).
        let get_weather = &tools[0]["function"];
        assert!(get_weather.get("strict").map(|v| v.is_null() || v == &serde_json::json!(false)).unwrap_or(true));
    }

    #[test]
    fn apply_strict_no_match_does_not_panic() {
        let mut tools: Vec<serde_json::Value> = vec![serde_json::json!({
            "type": "function",
            "function": {"name": "foo", "parameters": {"type": "string"}}
        })];
        // Should not panic when tool name is not found.
        apply_strict_to_forced_tool(&mut tools, "nonexistent");
    }
}
