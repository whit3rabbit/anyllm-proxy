// Anthropic Messages API request/response types
// PLAN.md lines 64-76, 667-697

use serde::{Deserialize, Serialize};

// --- Request types ---

/// Anthropic Messages API request body.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MessageCreateRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(default)]
    pub messages: Vec<InputMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<System>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Forward-compatible extension fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// System prompt: plain string or array of text blocks (with optional cache_control).
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum System {
    Text(String),
    Blocks(Vec<SystemBlock>),
}

/// System prompt text block with optional cache control.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String, // always "text"
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// Cache control directive for prompt caching.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
}

/// A single message in the conversation (user or assistant).
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct InputMessage {
    pub role: Role,
    pub content: Content,
}

/// Message role: user or assistant.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Message content: plain string or array of typed content blocks.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Typed content block within a message.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "document")]
    Document {
        source: DocumentSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<ToolResultContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Redacted thinking block: encrypted content returned when safety systems
    /// flag extended thinking. Must be passed back to the API for continuity.
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
}

/// Tool result content: plain string or array of content blocks.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Image source for image content blocks (base64 or URL).
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64" or "url"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Document source for PDF content blocks (base64).
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DocumentSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String, // "application/pdf"
    pub data: String,       // base64-encoded data
}

// --- Tool types ---

/// Tool definition with name, description, and JSON schema.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// How the model should use tools: auto, any, none, or specific tool.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "tool")]
    Tool { name: String },
}

/// Request metadata for abuse detection.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Extended thinking configuration.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    #[serde(rename = "enabled")]
    Enabled { budget_tokens: u32 },
    #[serde(rename = "disabled")]
    Disabled,
}

// --- Response types ---

/// Anthropic Messages API response body.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MessageResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String, // always "message"
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
}

/// Why the model stopped generating.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
}

/// Token usage counts for the request and response.
///
/// See <https://docs.anthropic.com/en/api/messages>
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn deserialize_basic_text_request() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello, world"}
            ]
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        assert_eq!(req.model, "claude-3-5-sonnet-20241022");
        assert_eq!(req.max_tokens, 1024);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        match &req.messages[0].content {
            Content::Text(s) => assert_eq!(s, "Hello, world"),
            _ => panic!("expected Content::Text"),
        }
    }

    #[test]
    fn deserialize_system_as_string() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 256,
            "messages": [],
            "system": "You are a helpful assistant."
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        match req.system.unwrap() {
            System::Text(s) => assert_eq!(s, "You are a helpful assistant."),
            _ => panic!("expected System::Text"),
        }
    }

    #[test]
    fn deserialize_system_as_blocks() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 256,
            "messages": [],
            "system": [
                {"type": "text", "text": "Be concise."},
                {"type": "text", "text": "Respond in JSON.", "cache_control": {"type": "ephemeral"}}
            ]
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        match req.system.unwrap() {
            System::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                assert_eq!(blocks[0].text, "Be concise.");
                assert!(blocks[0].cache_control.is_none());
                assert_eq!(blocks[1].text, "Respond in JSON.");
                assert_eq!(
                    blocks[1].cache_control.as_ref().unwrap().cache_type,
                    "ephemeral"
                );
            }
            _ => panic!("expected System::Blocks"),
        }
    }

    #[test]
    fn deserialize_tools_and_tool_choice() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What is the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a location",
                "input_schema": {
                    "type": "object",
                    "properties": {"location": {"type": "string"}},
                    "required": ["location"]
                }
            }],
            "tool_choice": {"type": "auto"}
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
        assert!(tools[0].description.is_some());
        match req.tool_choice.unwrap() {
            ToolChoice::Auto => {}
            other => panic!("expected ToolChoice::Auto, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_tool_choice_specific_tool() {
        let j = json!({"type": "tool", "name": "get_weather"});
        let tc: ToolChoice = serde_json::from_value(j).unwrap();
        match tc {
            ToolChoice::Tool { name } => assert_eq!(name, "get_weather"),
            other => panic!("expected ToolChoice::Tool, got {:?}", other),
        }
    }

    #[test]
    fn content_as_string_vs_blocks() {
        // String form
        let j = json!("just a string");
        let c: Content = serde_json::from_value(j).unwrap();
        match c {
            Content::Text(s) => assert_eq!(s, "just a string"),
            _ => panic!("expected Content::Text"),
        }

        // Blocks form
        let j = json!([{"type": "text", "text": "hello"}]);
        let c: Content = serde_json::from_value(j).unwrap();
        match c {
            Content::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "hello"),
                    _ => panic!("expected ContentBlock::Text"),
                }
            }
            _ => panic!("expected Content::Blocks"),
        }
    }

    #[test]
    fn deserialize_tool_use_block() {
        let j = json!({
            "type": "tool_use",
            "id": "toolu_01A09q90qw90lq917835lqs136",
            "name": "get_weather",
            "input": {"location": "San Francisco, CA"}
        });
        let block: ContentBlock = serde_json::from_value(j).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01A09q90qw90lq917835lqs136");
                assert_eq!(name, "get_weather");
                assert_eq!(input["location"], "San Francisco, CA");
            }
            _ => panic!("expected ContentBlock::ToolUse"),
        }
    }

    #[test]
    fn deserialize_tool_result_block() {
        let j = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_01A09q90qw90lq917835lqs136",
            "content": "72°F, sunny"
        });
        let block: ContentBlock = serde_json::from_value(j).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_01A09q90qw90lq917835lqs136");
                match content.unwrap() {
                    ToolResultContent::Text(s) => assert_eq!(s, "72°F, sunny"),
                    _ => panic!("expected ToolResultContent::Text"),
                }
                assert!(is_error.is_none());
            }
            _ => panic!("expected ContentBlock::ToolResult"),
        }
    }

    #[test]
    fn deserialize_tool_result_error() {
        let j = json!({
            "type": "tool_result",
            "tool_use_id": "toolu_err",
            "content": "something went wrong",
            "is_error": true
        });
        let block: ContentBlock = serde_json::from_value(j).unwrap();
        match block {
            ContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(is_error, Some(true));
            }
            _ => panic!("expected ContentBlock::ToolResult"),
        }
    }

    #[test]
    fn message_response_round_trip() {
        let resp = MessageResponse {
            id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
            response_type: "message".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "Hello!".into(),
            }],
            model: "claude-3-5-sonnet-20241022".into(),
            stop_reason: Some(StopReason::EndTurn),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            created: None,
        };
        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: MessageResponse = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.id, resp.id);
        assert_eq!(deserialized.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(deserialized.usage.input_tokens, 10);
        assert_eq!(deserialized.usage.output_tokens, 5);
    }

    #[test]
    fn reject_missing_max_tokens() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": []
        });
        let result = serde_json::from_value::<MessageCreateRequest>(j);
        assert!(result.is_err(), "should fail without max_tokens");
    }

    #[test]
    fn extra_fields_captured() {
        let j = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 100,
            "messages": [],
            "top_k": 40,
            "unknown_field": "value"
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        assert_eq!(req.top_k, Some(40));
        assert!(req.extra.get("top_k").is_none());
        assert_eq!(req.extra.get("unknown_field").unwrap(), &json!("value"));
    }

    #[test]
    fn stop_reason_variants() {
        assert_eq!(
            serde_json::from_value::<StopReason>(json!("end_turn")).unwrap(),
            StopReason::EndTurn,
        );
        assert_eq!(
            serde_json::from_value::<StopReason>(json!("max_tokens")).unwrap(),
            StopReason::MaxTokens,
        );
        assert_eq!(
            serde_json::from_value::<StopReason>(json!("stop_sequence")).unwrap(),
            StopReason::StopSequence,
        );
        assert_eq!(
            serde_json::from_value::<StopReason>(json!("tool_use")).unwrap(),
            StopReason::ToolUse,
        );
    }

    #[test]
    fn usage_optional_cache_fields_omitted() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let j = serde_json::to_value(&usage).unwrap();
        assert!(!j
            .as_object()
            .unwrap()
            .contains_key("cache_creation_input_tokens"));
        assert!(!j
            .as_object()
            .unwrap()
            .contains_key("cache_read_input_tokens"));
    }

    #[test]
    fn thinking_config_enabled_roundtrip() {
        let cfg = ThinkingConfig::Enabled {
            budget_tokens: 8192,
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json, json!({"type": "enabled", "budget_tokens": 8192}));
        let parsed: ThinkingConfig = serde_json::from_value(json).unwrap();
        assert!(matches!(
            parsed,
            ThinkingConfig::Enabled {
                budget_tokens: 8192
            }
        ));
    }

    #[test]
    fn thinking_config_disabled_roundtrip() {
        let cfg = ThinkingConfig::Disabled;
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json, json!({"type": "disabled"}));
        let parsed: ThinkingConfig = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, ThinkingConfig::Disabled));
    }

    #[test]
    fn thinking_content_block_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me reason about this...".into(),
            signature: Some("sig_abc".into()),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "thinking");
        assert_eq!(json["thinking"], "Let me reason about this...");
        assert_eq!(json["signature"], "sig_abc");
        let parsed: ContentBlock = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, ContentBlock::Thinking { .. }));
    }

    #[test]
    fn request_with_thinking_deserializes() {
        let j = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "messages": [],
            "thinking": {"type": "enabled", "budget_tokens": 4096}
        });
        let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
        assert!(matches!(
            req.thinking,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 4096
            })
        ));
    }

    #[test]
    fn deserialize_redacted_thinking_block() {
        let j = json!({
            "type": "redacted_thinking",
            "data": "EqQBCgIYAhIM1gbcDa9GJwZA2b3h"
        });
        let block: ContentBlock = serde_json::from_value(j).unwrap();
        match block {
            ContentBlock::RedactedThinking { data } => {
                assert_eq!(data, "EqQBCgIYAhIM1gbcDa9GJwZA2b3h");
            }
            _ => panic!("expected ContentBlock::RedactedThinking"),
        }
    }

    #[test]
    fn redacted_thinking_round_trip() {
        let block = ContentBlock::RedactedThinking {
            data: "encrypted_data_here".into(),
        };
        let serialized = serde_json::to_string(&block).unwrap();
        assert!(serialized.contains("\"redacted_thinking\""));
        let deserialized: ContentBlock = serde_json::from_str(&serialized).unwrap();
        match deserialized {
            ContentBlock::RedactedThinking { data } => {
                assert_eq!(data, "encrypted_data_here");
            }
            _ => panic!("expected ContentBlock::RedactedThinking"),
        }
    }
}
