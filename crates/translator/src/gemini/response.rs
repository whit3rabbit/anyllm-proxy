// Gemini generateContent API response types.
//
// Shared `Content` and `Part` types are imported from `request.rs`.

use serde::{Deserialize, Serialize};

use super::request::Content;

/// Response from `/models/{model}:generateContent` (and streaming chunks).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
}

/// A single candidate response from the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    #[serde(default)]
    pub content: Content,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum FinishReason {
    STOP,
    MAX_TOKENS,
    SAFETY,
    RECITATION,
    LANGUAGE,
    OTHER,
    /// Catch-all for values added by the API in the future.
    #[serde(other)]
    Unknown,
}

/// Token usage metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    #[serde(default)]
    pub prompt_token_count: u32,
    #[serde(default)]
    pub candidates_token_count: u32,
    #[serde(default)]
    pub total_token_count: u32,
    #[serde(default)]
    pub cached_content_token_count: u32,
}

/// Per-category safety rating returned with each candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyRating {
    pub category: String,
    pub probability: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::request::Part;
    use serde_json::json;

    #[test]
    fn deserialize_basic_text_response() {
        let j = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello!"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 3,
                "totalTokenCount": 8
            }
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(
            resp.candidates[0].content.parts[0].text.as_deref(),
            Some("Hello!")
        );
        assert_eq!(resp.candidates[0].finish_reason, Some(FinishReason::STOP));
        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 5);
        assert_eq!(usage.candidates_token_count, 3);
        assert_eq!(usage.total_token_count, 8);
    }

    #[test]
    fn deserialize_tool_call_response() {
        let j = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "args": {"city": "London"}
                        }
                    }]
                },
                "finishReason": "STOP"
            }]
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        let fc = resp.candidates[0].content.parts[0]
            .function_call
            .as_ref()
            .unwrap();
        assert_eq!(fc.name, "get_weather");
        assert_eq!(fc.args["city"], "London");
    }

    #[test]
    fn deserialize_usage_metadata() {
        let j = json!({
            "candidates": [],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30,
                "cachedContentTokenCount": 5
            }
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        let u = resp.usage_metadata.unwrap();
        assert_eq!(u.prompt_token_count, 10);
        assert_eq!(u.cached_content_token_count, 5);
    }

    #[test]
    fn finish_reason_stop_and_max_tokens() {
        let stop: FinishReason = serde_json::from_value(json!("STOP")).unwrap();
        let max: FinishReason = serde_json::from_value(json!("MAX_TOKENS")).unwrap();
        assert_eq!(stop, FinishReason::STOP);
        assert_eq!(max, FinishReason::MAX_TOKENS);
    }

    #[test]
    fn finish_reason_unknown_variant() {
        let fr: FinishReason = serde_json::from_value(json!("BLOCKLIST")).unwrap();
        assert_eq!(fr, FinishReason::Unknown);
    }

    #[test]
    fn empty_candidates() {
        let j = json!({"candidates": []});
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        assert!(resp.candidates.is_empty());
        assert!(resp.usage_metadata.is_none());
    }

    #[test]
    fn candidate_without_finish_reason() {
        let j = json!({
            "candidates": [{
                "content": {"parts": [{"text": "partial"}]}
            }]
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        assert!(resp.candidates[0].finish_reason.is_none());
    }

    #[test]
    fn safety_rating_deserialization() {
        let j = json!({
            "candidates": [{
                "content": {"parts": [{"text": "ok"}]},
                "safetyRatings": [{
                    "category": "HARM_CATEGORY_HATE_SPEECH",
                    "probability": "NEGLIGIBLE"
                }]
            }]
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        let sr = &resp.candidates[0].safety_ratings.as_ref().unwrap()[0];
        assert_eq!(sr.category, "HARM_CATEGORY_HATE_SPEECH");
        assert_eq!(sr.probability, "NEGLIGIBLE");
    }

    #[test]
    fn round_trip_response() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::text("hi")],
                },
                finish_reason: Some(FinishReason::STOP),
                safety_ratings: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 1,
                candidates_token_count: 1,
                total_token_count: 2,
                cached_content_token_count: 0,
            }),
            model_version: Some("gemini-2.0-flash".into()),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let back: GenerateContentResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.candidates.len(), 1);
        assert_eq!(back.candidates[0].finish_reason, Some(FinishReason::STOP));
        assert_eq!(back.model_version.as_deref(), Some("gemini-2.0-flash"));
    }

    #[test]
    fn streaming_partial_content() {
        // Streaming chunks use the same type but may have partial content
        let j = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello"}]
                }
            }]
        });
        let resp: GenerateContentResponse = serde_json::from_value(j).unwrap();
        assert_eq!(
            resp.candidates[0].content.parts[0].text.as_deref(),
            Some("Hello")
        );
        assert!(resp.candidates[0].finish_reason.is_none());
        assert!(resp.usage_metadata.is_none());
    }
}
