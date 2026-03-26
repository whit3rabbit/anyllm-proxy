//! Builder helpers for Anthropic tool definitions and tool choice.
//!
//! These builders produce [`Tool`] and [`ToolChoice`] values from
//! `anyllm_translate::anthropic` with a fluent API, avoiding raw JSON
//! construction for common cases.

use anyllm_translate::anthropic::{Tool, ToolChoice};
use serde_json::Value;

/// Fluent builder for an Anthropic [`Tool`] definition.
///
/// # Examples
///
/// ```
/// use anyllm_client::ToolBuilder;
/// use serde_json::json;
///
/// let tool = ToolBuilder::new("get_weather")
///     .description("Get the current weather for a location")
///     .input_schema(json!({
///         "type": "object",
///         "properties": {
///             "location": { "type": "string" }
///         },
///         "required": ["location"]
///     }))
///     .build();
///
/// assert_eq!(tool.name, "get_weather");
/// ```
pub struct ToolBuilder {
    name: String,
    description: Option<String>,
    input_schema: Value,
}

impl ToolBuilder {
    /// Start building a tool with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            input_schema: Value::Object(serde_json::Map::new()),
        }
    }

    /// Set the human-readable description shown to the model.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Set the JSON Schema describing the tool's expected input.
    pub fn input_schema(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Consume the builder and produce a [`Tool`].
    pub fn build(self) -> Tool {
        Tool {
            name: self.name,
            description: self.description,
            input_schema: self.input_schema,
        }
    }
}

/// Convenience constructors for [`ToolChoice`] variants.
///
/// # Examples
///
/// ```
/// use anyllm_client::ToolChoiceBuilder;
///
/// let choice = ToolChoiceBuilder::auto();
/// let specific = ToolChoiceBuilder::specific("get_weather");
/// ```
pub struct ToolChoiceBuilder;

impl ToolChoiceBuilder {
    /// Let the model decide whether to use tools.
    pub fn auto() -> ToolChoice {
        ToolChoice::Auto {
            disable_parallel_tool_use: None,
        }
    }

    /// Force the model to use at least one tool.
    pub fn any() -> ToolChoice {
        ToolChoice::Any {
            disable_parallel_tool_use: None,
        }
    }

    /// Prevent the model from using any tools.
    pub fn none() -> ToolChoice {
        ToolChoice::None
    }

    /// Force the model to use a specific tool by name.
    pub fn specific(name: &str) -> ToolChoice {
        ToolChoice::Tool {
            name: name.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_builder_minimal() {
        let tool = ToolBuilder::new("test_tool").build();
        assert_eq!(tool.name, "test_tool");
        assert!(tool.description.is_none());
        assert!(tool.input_schema.is_object());
    }

    #[test]
    fn tool_builder_full() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });

        let tool = ToolBuilder::new("search")
            .description("Search the web")
            .input_schema(schema.clone())
            .build();

        assert_eq!(tool.name, "search");
        assert_eq!(tool.description.as_deref(), Some("Search the web"));
        assert_eq!(tool.input_schema, schema);
    }

    #[test]
    fn tool_choice_auto() {
        let choice = ToolChoiceBuilder::auto();
        assert_eq!(
            choice,
            ToolChoice::Auto {
                disable_parallel_tool_use: None
            }
        );
    }

    #[test]
    fn tool_choice_any() {
        let choice = ToolChoiceBuilder::any();
        assert_eq!(
            choice,
            ToolChoice::Any {
                disable_parallel_tool_use: None
            }
        );
    }

    #[test]
    fn tool_choice_none() {
        let choice = ToolChoiceBuilder::none();
        assert_eq!(choice, ToolChoice::None);
    }

    #[test]
    fn tool_choice_specific() {
        let choice = ToolChoiceBuilder::specific("get_weather");
        assert_eq!(
            choice,
            ToolChoice::Tool {
                name: "get_weather".to_string()
            }
        );
    }

    #[test]
    fn tool_serializes_correctly() {
        let tool = ToolBuilder::new("calc")
            .description("Calculator")
            .input_schema(json!({"type": "object"}))
            .build();

        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["name"], "calc");
        assert_eq!(json["description"], "Calculator");
    }
}
