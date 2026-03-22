use serde::{Deserialize, Serialize};

/// Wrapper containing function declarations.
///
/// See <https://ai.google.dev/api/caching#Tool>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_declarations: Option<Vec<FunctionDeclaration>>,
}

/// A single function declaration for tool calling.
///
/// Parameters must conform to the OpenAPI 3.0 schema subset that Gemini
/// accepts. Unsupported keys ($schema, $ref, $defs, default, pattern, etc.)
/// must be stripped before sending. See Phase 20d (schema sanitizer).
///
/// See <https://ai.google.dev/api/caching#FunctionDeclaration>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDeclaration {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema subset (OpenAPI 3.0). Use serde_json::Value since the
    /// schema structure varies and Gemini imposes its own restrictions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// Configuration for how the model should use tools.
///
/// See <https://ai.google.dev/api/caching#ToolConfig>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_calling_config: Option<FunctionCallingConfig>,
}

/// Controls function calling behavior.
///
/// See <https://ai.google.dev/api/caching#FunctionCallingConfig>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<FunctionCallingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum FunctionCallingMode {
    #[serde(rename = "AUTO")]
    Auto,
    #[serde(rename = "NONE")]
    None,
    #[serde(rename = "ANY")]
    Any,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_with_function_declarations() {
        let raw = json!({
            "functionDeclarations": [{
                "name": "get_weather",
                "description": "Get current weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }
            }]
        });
        let tool: Tool = serde_json::from_value(raw).unwrap();
        let decls = tool.function_declarations.as_ref().unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "get_weather");
        assert_eq!(decls[0].description.as_deref(), Some("Get current weather"));
        assert!(decls[0].parameters.is_some());

        // roundtrip
        let json_str = serde_json::to_string(&tool).unwrap();
        let _: Tool = serde_json::from_str(&json_str).unwrap();
    }

    #[test]
    fn tool_config_auto_mode() {
        let raw = json!({
            "functionCallingConfig": {
                "mode": "AUTO"
            }
        });
        let config: ToolConfig = serde_json::from_value(raw).unwrap();
        let fcc = config.function_calling_config.unwrap();
        assert_eq!(fcc.mode, Some(FunctionCallingMode::Auto));
        assert!(fcc.allowed_function_names.is_none());
    }

    #[test]
    fn tool_config_any_with_allowed_names() {
        let raw = json!({
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": ["get_weather", "search"]
            }
        });
        let config: ToolConfig = serde_json::from_value(raw).unwrap();
        let fcc = config.function_calling_config.unwrap();
        assert_eq!(fcc.mode, Some(FunctionCallingMode::Any));
        let names = fcc.allowed_function_names.unwrap();
        assert_eq!(names, vec!["get_weather", "search"]);
    }

    #[test]
    fn function_calling_mode_variants() {
        for (s, expected) in [
            ("\"AUTO\"", FunctionCallingMode::Auto),
            ("\"NONE\"", FunctionCallingMode::None),
            ("\"ANY\"", FunctionCallingMode::Any),
        ] {
            let mode: FunctionCallingMode = serde_json::from_str(s).unwrap();
            assert_eq!(mode, expected);
        }
    }

    #[test]
    fn tool_minimal() {
        // Tool with no function declarations (empty wrapper)
        let raw = json!({});
        let tool: Tool = serde_json::from_value(raw).unwrap();
        assert!(tool.function_declarations.is_none());
    }

    #[test]
    fn function_declaration_minimal() {
        let raw = json!({"name": "noop"});
        let decl: FunctionDeclaration = serde_json::from_value(raw).unwrap();
        assert_eq!(decl.name, "noop");
        assert!(decl.description.is_none());
        assert!(decl.parameters.is_none());
    }
}
