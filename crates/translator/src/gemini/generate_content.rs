use serde::{Deserialize, Serialize};

use super::tools::{Tool, ToolConfig};

/// Gemini generateContent request.
///
/// See <https://ai.google.dev/api/generate-content#v1beta.models.generateContent>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A message containing role and content parts.
///
/// Role is optional because `systemInstruction` content has no role field.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<GeminiRole>,
    pub parts: Vec<Part>,
}

/// Gemini roles: only `user` and `model` are valid.
///
/// Anthropic `assistant` maps to `model`. There is no `system` or `developer`
/// role; system prompts go in the separate `systemInstruction` field.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GeminiRole {
    User,
    Model,
}

/// A single content part. Gemini uses a union type distinguished by which
/// field is present (not a type tag).
///
/// See <https://ai.google.dev/api/caching#Part>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Part {
    /// Struct variants before Text to prevent the single-field Text variant
    /// from greedily matching during untagged deserialization.
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallData,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponseData,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineData,
    },
    FileData {
        #[serde(rename = "fileData")]
        file_data: FileData,
    },
    Text {
        text: String,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub mime_type: String,
    pub file_uri: String,
}

/// Unlike OpenAI where `arguments` is a JSON string, Gemini `args` is a
/// JSON object.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallData {
    pub name: String,
    pub args: serde_json::Value,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponseData {
    pub name: String,
    pub response: serde_json::Value,
}

/// Generation parameters.
///
/// See <https://ai.google.dev/api/generate-content#v1beta.GenerationConfig>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Safety setting to control content filtering thresholds.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    pub category: HarmCategory,
    pub threshold: HarmBlockThreshold,
}

/// Harm categories for safety settings and ratings.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum HarmCategory {
    #[serde(rename = "HARM_CATEGORY_HATE_SPEECH")]
    HateSpeech,
    #[serde(rename = "HARM_CATEGORY_DANGEROUS_CONTENT")]
    DangerousContent,
    #[serde(rename = "HARM_CATEGORY_HARASSMENT")]
    Harassment,
    #[serde(rename = "HARM_CATEGORY_SEXUALLY_EXPLICIT")]
    SexuallyExplicit,
}

/// Threshold levels for blocking harmful content.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum HarmBlockThreshold {
    #[serde(rename = "BLOCK_LOW_AND_ABOVE")]
    BlockLowAndAbove,
    #[serde(rename = "BLOCK_MEDIUM_AND_ABOVE")]
    BlockMediumAndAbove,
    #[serde(rename = "BLOCK_ONLY_HIGH")]
    BlockOnlyHigh,
    #[serde(rename = "BLOCK_NONE")]
    BlockNone,
    #[serde(rename = "OFF")]
    Off,
}

/// Gemini generateContent response.
///
/// Candidates may be absent if the prompt was blocked by safety filters.
///
/// See <https://ai.google.dev/api/generate-content#v1beta.GenerateContentResponse>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<Candidate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<PromptFeedback>,
}

/// A single candidate response.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<CitationMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// Reason the model stopped generating.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum FinishReason {
    #[serde(rename = "STOP")]
    Stop,
    #[serde(rename = "MAX_TOKENS")]
    MaxTokens,
    #[serde(rename = "SAFETY")]
    Safety,
    #[serde(rename = "RECITATION")]
    Recitation,
    #[serde(rename = "OTHER")]
    Other,
    #[serde(rename = "BLOCKLIST")]
    Blocklist,
    #[serde(rename = "PROHIBITED_CONTENT")]
    ProhibitedContent,
    #[serde(rename = "SPII")]
    Spii,
    #[serde(rename = "MALFORMED_FUNCTION_CALL")]
    MalformedFunctionCall,
}

/// Safety rating for a specific harm category.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SafetyRating {
    pub category: HarmCategory,
    /// Probability level: NEGLIGIBLE, LOW, MEDIUM, HIGH.
    /// Kept as String for forward compatibility.
    pub probability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

/// Token usage metadata.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidates_token_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<u32>,
}

/// Feedback about the prompt (e.g., safety blocking before generation).
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

/// Citation metadata for a candidate.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_sources: Option<Vec<CitationSource>>,
}

/// A single citation source reference.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CitationSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_basic_request() {
        let raw = json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": "Hello"}]
            }],
            "generationConfig": {
                "maxOutputTokens": 256,
                "temperature": 0.7
            }
        });
        let req: GenerateContentRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, Some(GeminiRole::User));
        let config = req.generation_config.unwrap();
        assert_eq!(config.max_output_tokens, Some(256));
        assert_eq!(config.temperature, Some(0.7));
    }

    #[test]
    fn deserialize_request_with_system_instruction() {
        let raw = json!({
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "systemInstruction": {
                "parts": [{"text": "You are helpful."}]
            }
        });
        let req: GenerateContentRequest = serde_json::from_value(raw).unwrap();
        let sys = req.system_instruction.unwrap();
        assert!(sys.role.is_none());
        match &sys.parts[0] {
            Part::Text { text } => assert_eq!(text, "You are helpful."),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn part_text_roundtrip() {
        let part = Part::Text {
            text: "hello".into(),
        };
        let json_str = serde_json::to_string(&part).unwrap();
        let roundtrip: Part = serde_json::from_str(&json_str).unwrap();
        match roundtrip {
            Part::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn part_inline_data_roundtrip() {
        let part = Part::InlineData {
            inline_data: InlineData {
                mime_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        };
        let val = serde_json::to_value(&part).unwrap();
        assert_eq!(val["inlineData"]["mimeType"], "image/png");

        let roundtrip: Part = serde_json::from_value(val).unwrap();
        match roundtrip {
            Part::InlineData { inline_data } => {
                assert_eq!(inline_data.mime_type, "image/png");
                assert_eq!(inline_data.data, "iVBORw0KGgo=");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn part_file_data_roundtrip() {
        let part = Part::FileData {
            file_data: FileData {
                mime_type: "application/pdf".into(),
                file_uri: "gs://bucket/file.pdf".into(),
            },
        };
        let val = serde_json::to_value(&part).unwrap();
        assert_eq!(val["fileData"]["fileUri"], "gs://bucket/file.pdf");

        let roundtrip: Part = serde_json::from_value(val).unwrap();
        match roundtrip {
            Part::FileData { file_data } => {
                assert_eq!(file_data.file_uri, "gs://bucket/file.pdf");
            }
            _ => panic!("expected FileData"),
        }
    }

    #[test]
    fn part_function_call_roundtrip() {
        let part = Part::FunctionCall {
            function_call: FunctionCallData {
                name: "get_weather".into(),
                args: json!({"location": "NYC"}),
            },
        };
        let val = serde_json::to_value(&part).unwrap();
        assert_eq!(val["functionCall"]["name"], "get_weather");
        // args is a JSON object, not a string
        assert_eq!(val["functionCall"]["args"]["location"], "NYC");

        let roundtrip: Part = serde_json::from_value(val).unwrap();
        match roundtrip {
            Part::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "get_weather");
                assert_eq!(function_call.args["location"], "NYC");
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn part_function_response_roundtrip() {
        let part = Part::FunctionResponse {
            function_response: FunctionResponseData {
                name: "get_weather".into(),
                response: json!({"temperature": 72, "unit": "F"}),
            },
        };
        let val = serde_json::to_value(&part).unwrap();
        assert_eq!(val["functionResponse"]["name"], "get_weather");

        let roundtrip: Part = serde_json::from_value(val).unwrap();
        match roundtrip {
            Part::FunctionResponse { function_response } => {
                assert_eq!(function_response.name, "get_weather");
                assert_eq!(function_response.response["temperature"], 72);
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn deserialize_basic_response() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello! How can I help?"}]
                },
                "finishReason": "STOP",
                "index": 0
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 8,
                "totalTokenCount": 18
            }
        });
        let resp: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        let candidates = resp.candidates.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(candidates[0].index, Some(0));

        let content = candidates[0].content.as_ref().unwrap();
        assert_eq!(content.role, Some(GeminiRole::Model));

        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, Some(10));
        assert_eq!(usage.candidates_token_count, Some(8));
        assert_eq!(usage.total_token_count, Some(18));
    }

    #[test]
    fn deserialize_response_with_tool_calls() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "search",
                            "args": {"query": "rust serde"}
                        }
                    }]
                },
                "finishReason": "STOP"
            }]
        });
        let resp: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        let candidates = resp.candidates.unwrap();
        let parts = &candidates[0].content.as_ref().unwrap().parts;
        match &parts[0] {
            Part::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "search");
                assert_eq!(function_call.args["query"], "rust serde");
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn deserialize_safety_blocked_response() {
        let raw = json!({
            "promptFeedback": {
                "blockReason": "SAFETY",
                "safetyRatings": [{
                    "category": "HARM_CATEGORY_DANGEROUS_CONTENT",
                    "probability": "HIGH",
                    "blocked": true
                }]
            }
        });
        let resp: GenerateContentResponse = serde_json::from_value(raw).unwrap();
        assert!(resp.candidates.is_none());
        let feedback = resp.prompt_feedback.unwrap();
        assert_eq!(feedback.block_reason.as_deref(), Some("SAFETY"));
        let ratings = feedback.safety_ratings.unwrap();
        assert_eq!(ratings[0].category, HarmCategory::DangerousContent);
        assert_eq!(ratings[0].probability, "HIGH");
        assert_eq!(ratings[0].blocked, Some(true));
    }

    #[test]
    fn finish_reason_variants() {
        let variants = [
            ("\"STOP\"", FinishReason::Stop),
            ("\"MAX_TOKENS\"", FinishReason::MaxTokens),
            ("\"SAFETY\"", FinishReason::Safety),
            ("\"RECITATION\"", FinishReason::Recitation),
            ("\"OTHER\"", FinishReason::Other),
            ("\"BLOCKLIST\"", FinishReason::Blocklist),
            ("\"PROHIBITED_CONTENT\"", FinishReason::ProhibitedContent),
            ("\"SPII\"", FinishReason::Spii),
            (
                "\"MALFORMED_FUNCTION_CALL\"",
                FinishReason::MalformedFunctionCall,
            ),
        ];
        for (s, expected) in variants {
            let reason: FinishReason = serde_json::from_str(s).unwrap();
            assert_eq!(reason, expected);
        }
    }

    #[test]
    fn role_variants() {
        let user: GeminiRole = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(user, GeminiRole::User);
        let model: GeminiRole = serde_json::from_str("\"model\"").unwrap();
        assert_eq!(model, GeminiRole::Model);
    }

    #[test]
    fn serialize_request_camel_case() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some(GeminiRole::User),
                parts: vec![Part::Text { text: "Hi".into() }],
            }],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: Some(GenerationConfig {
                max_output_tokens: Some(100),
                temperature: None,
                top_p: None,
                top_k: None,
                candidate_count: None,
                stop_sequences: None,
                seed: None,
                presence_penalty: None,
                frequency_penalty: None,
                response_mime_type: None,
                response_schema: None,
                extra: serde_json::Map::new(),
            }),
            safety_settings: None,
            extra: serde_json::Map::new(),
        };
        let val = serde_json::to_value(&req).unwrap();
        // Verify camelCase keys
        assert!(val.get("generationConfig").is_some());
        assert!(val.get("generation_config").is_none());
        let gc = val.get("generationConfig").unwrap();
        assert!(gc.get("maxOutputTokens").is_some());
        assert!(gc.get("max_output_tokens").is_none());
    }

    #[test]
    fn extra_fields_captured() {
        let raw = json!({
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "cachedContent": "projects/123/cachedContents/abc"
        });
        let req: GenerateContentRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(
            req.extra.get("cachedContent").and_then(|v| v.as_str()),
            Some("projects/123/cachedContents/abc")
        );
    }

    #[test]
    fn usage_metadata_partial() {
        let raw = json!({"promptTokenCount": 42});
        let usage: UsageMetadata = serde_json::from_value(raw).unwrap();
        assert_eq!(usage.prompt_token_count, Some(42));
        assert!(usage.candidates_token_count.is_none());
        assert!(usage.total_token_count.is_none());
    }

    #[test]
    fn harm_category_roundtrip() {
        let setting = SafetySetting {
            category: HarmCategory::HateSpeech,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        };
        let val = serde_json::to_value(&setting).unwrap();
        assert_eq!(val["category"], "HARM_CATEGORY_HATE_SPEECH");
        assert_eq!(val["threshold"], "BLOCK_ONLY_HIGH");

        let roundtrip: SafetySetting = serde_json::from_value(val).unwrap();
        assert_eq!(roundtrip.category, HarmCategory::HateSpeech);
        assert_eq!(roundtrip.threshold, HarmBlockThreshold::BlockOnlyHigh);
    }

    #[test]
    fn fixture_basic() {
        let fixture = include_str!("../../../../fixtures/gemini/generate_content_basic.json");
        let parsed: serde_json::Value = serde_json::from_str(fixture).unwrap();

        let req: GenerateContentRequest =
            serde_json::from_value(parsed["request"].clone()).unwrap();
        assert_eq!(req.contents.len(), 1);

        let resp: GenerateContentResponse =
            serde_json::from_value(parsed["response"].clone()).unwrap();
        let candidates = resp.candidates.unwrap();
        assert_eq!(candidates[0].finish_reason, Some(FinishReason::Stop));
    }

    #[test]
    fn fixture_tool_call() {
        let fixture = include_str!("../../../../fixtures/gemini/generate_content_tool_call.json");
        let parsed: serde_json::Value = serde_json::from_str(fixture).unwrap();

        let req: GenerateContentRequest =
            serde_json::from_value(parsed["request"].clone()).unwrap();
        let tools = req.tools.unwrap();
        assert!(!tools.is_empty());

        let resp: GenerateContentResponse =
            serde_json::from_value(parsed["response"].clone()).unwrap();
        let candidates = resp.candidates.unwrap();
        let parts = &candidates[0].content.as_ref().unwrap().parts;
        match &parts[0] {
            Part::FunctionCall { function_call } => {
                assert!(!function_call.name.is_empty());
            }
            _ => panic!("expected FunctionCall in fixture response"),
        }
    }
}
