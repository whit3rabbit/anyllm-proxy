// Gemini generateContent API request types.
//
// Pure data types with serde, no IO. Field names use camelCase to match
// the Gemini REST API (https://ai.google.dev/api/generate-content).

use serde::{Deserialize, Serialize};

// --- Request types ---

/// POST body for `/models/{model}:generateContent` and `:streamGenerateContent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
}

/// A conversation turn: role + parts.
///
/// `role` is optional because `systemInstruction` omits it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Content {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default)]
    pub parts: Vec<Part>,
}

/// A single content part. Gemini discriminates by field presence, so we use
/// a struct with optional fields rather than an enum.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FileData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCallData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponseData>,
    /// True for thought parts produced by thinking models (e.g., Gemini 2.5 Pro/Flash).
    /// Never set in requests; only appears in model responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
}

impl Part {
    pub fn text(t: impl Into<String>) -> Self {
        Self {
            text: Some(t.into()),
            ..Default::default()
        }
    }

    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            function_call: Some(FunctionCallData {
                name: name.into(),
                args,
            }),
            ..Default::default()
        }
    }

    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self {
            function_response: Some(FunctionResponseData {
                name: name.into(),
                response,
            }),
            ..Default::default()
        }
    }

    pub fn inline_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            inline_data: Some(InlineData {
                mime_type: mime_type.into(),
                data: data.into(),
            }),
            ..Default::default()
        }
    }
}

/// Base64-encoded inline binary data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    /// Base64-encoded bytes.
    pub data: String,
}

/// Reference to a file stored via the Gemini File API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub mime_type: String,
    pub file_uri: String,
}

/// A function call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallData {
    pub name: String,
    /// JSON object (not a string like OpenAI).
    pub args: serde_json::Value,
}

/// The result of executing a function, sent back to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponseData {
    pub name: String,
    pub response: serde_json::Value,
}

/// Sampling and output configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
    /// Configures extended thinking for supported models (e.g., Gemini 2.5 Pro/Flash).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}

/// Configures extended thinking for Gemini 2.5 thinking models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Token budget for the thinking phase. Maps from Anthropic `budget_tokens`.
    pub thinking_budget: u32,
    /// Whether to include thought parts in the response. Set to `true` to
    /// surface thinking content as `ContentBlock::Thinking` in the translation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
}

/// Wrapper for function declarations provided to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// A single function the model may call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// Controls how and whether the model calls functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    pub function_calling_config: FunctionCallingConfig,
}

/// Function calling mode: AUTO, NONE, or ANY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallingConfig {
    pub mode: String,
}

/// Per-category safety threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySetting {
    pub category: String,
    pub threshold: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialize_basic_request() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::text("Hello")],
            }],
            system_instruction: None,
            generation_config: None,
            tools: None,
            tool_config: None,
            safety_settings: None,
        };
        let j = serde_json::to_value(&req).unwrap();
        assert_eq!(j["contents"][0]["role"], "user");
        assert_eq!(j["contents"][0]["parts"][0]["text"], "Hello");
        // Optional fields must be absent
        assert!(j.get("systemInstruction").is_none());
        assert!(j.get("tools").is_none());
    }

    #[test]
    fn serialize_with_system_instruction() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::text("You are helpful.")],
            }),
            generation_config: None,
            tools: None,
            tool_config: None,
            safety_settings: None,
        };
        let j = serde_json::to_value(&req).unwrap();
        assert_eq!(
            j["systemInstruction"]["parts"][0]["text"],
            "You are helpful."
        );
        // systemInstruction should not have a role field
        assert!(j["systemInstruction"].get("role").is_none());
    }

    #[test]
    fn serialize_with_tools() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: None,
            generation_config: None,
            tools: Some(vec![Tool {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: Some("Get weather".into()),
                    parameters: Some(
                        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
                    ),
                }],
            }]),
            tool_config: None,
            safety_settings: None,
        };
        let j = serde_json::to_value(&req).unwrap();
        assert_eq!(
            j["tools"][0]["functionDeclarations"][0]["name"],
            "get_weather"
        );
    }

    #[test]
    fn serialize_with_generation_config() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(1024),
                top_k: Some(40),
                ..Default::default()
            }),
            tools: None,
            tool_config: None,
            safety_settings: None,
        };
        let j = serde_json::to_value(&req).unwrap();
        let gc = &j["generationConfig"];
        let temp = gc["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001, "temperature was {temp}");
        assert_eq!(gc["maxOutputTokens"], 1024);
        assert_eq!(gc["topK"], 40);
        // Unset fields must be absent
        assert!(gc.get("topP").is_none());
        assert!(gc.get("seed").is_none());
    }

    #[test]
    fn part_text_constructor() {
        let p = Part::text("hello");
        assert_eq!(p.text.as_deref(), Some("hello"));
        assert!(p.inline_data.is_none());
        assert!(p.function_call.is_none());
    }

    #[test]
    fn part_function_call_constructor() {
        let p = Part::function_call("calc", json!({"expr": "1+1"}));
        let fc = p.function_call.unwrap();
        assert_eq!(fc.name, "calc");
        assert_eq!(fc.args, json!({"expr": "1+1"}));
        assert!(p.text.is_none());
    }

    #[test]
    fn part_function_response_constructor() {
        let p = Part::function_response("calc", json!({"result": 2}));
        let fr = p.function_response.unwrap();
        assert_eq!(fr.name, "calc");
        assert_eq!(fr.response, json!({"result": 2}));
    }

    #[test]
    fn part_inline_data_constructor() {
        let p = Part::inline_data("image/png", "iVBOR...");
        let id = p.inline_data.unwrap();
        assert_eq!(id.mime_type, "image/png");
        assert_eq!(id.data, "iVBOR...");
    }

    #[test]
    fn round_trip_request() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::text("Hi")],
            }],
            system_instruction: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            }),
            tools: None,
            tool_config: None,
            safety_settings: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let back: GenerateContentRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.contents.len(), 1);
        assert_eq!(back.generation_config.unwrap().temperature, Some(0.5));
    }

    #[test]
    fn generation_config_camel_case() {
        let gc = GenerationConfig {
            max_output_tokens: Some(100),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            ..Default::default()
        };
        let j = serde_json::to_value(&gc).unwrap();
        assert!(j.get("maxOutputTokens").is_some());
        assert!(j.get("stopSequences").is_some());
        assert!(j.get("responseMimeType").is_some());
        // snake_case keys must not appear
        assert!(j.get("max_output_tokens").is_none());
    }

    #[test]
    fn tool_config_serializes_correctly() {
        let tc = ToolConfig {
            function_calling_config: FunctionCallingConfig { mode: "ANY".into() },
        };
        let j = serde_json::to_value(&tc).unwrap();
        assert_eq!(j["functionCallingConfig"]["mode"], "ANY");
    }

    #[test]
    fn empty_optional_fields_omitted() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: None,
            generation_config: None,
            tools: None,
            tool_config: None,
            safety_settings: None,
        };
        let j = serde_json::to_value(&req).unwrap();
        let obj = j.as_object().unwrap();
        assert_eq!(obj.len(), 1); // only "contents"
    }

    #[test]
    fn content_with_user_and_model_roles() {
        let user = Content {
            role: Some("user".into()),
            parts: vec![Part::text("question")],
        };
        let model = Content {
            role: Some("model".into()),
            parts: vec![Part::text("answer")],
        };
        let j_user = serde_json::to_value(&user).unwrap();
        let j_model = serde_json::to_value(&model).unwrap();
        assert_eq!(j_user["role"], "user");
        assert_eq!(j_model["role"], "model");
    }

    #[test]
    fn function_call_args_is_json_object() {
        let fc = FunctionCallData {
            name: "f".into(),
            args: json!({"a": 1}),
        };
        assert!(fc.args.is_object());
        let j = serde_json::to_value(&fc).unwrap();
        assert!(j["args"].is_object());
    }

    #[test]
    fn safety_setting_serializes() {
        let ss = SafetySetting {
            category: "HARM_CATEGORY_DANGEROUS_CONTENT".into(),
            threshold: "BLOCK_ONLY_HIGH".into(),
        };
        let j = serde_json::to_value(&ss).unwrap();
        assert_eq!(j["category"], "HARM_CATEGORY_DANGEROUS_CONTENT");
        assert_eq!(j["threshold"], "BLOCK_ONLY_HIGH");
    }
}
