use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// An asynchronous tool that can be executed server-side.
pub trait Tool: Send + Sync {
    /// The unique name of the tool, matched against Anthropic's tool_use name.
    fn name(&self) -> &str;

    /// Description of the tool's behavior, passed to the LLM.
    fn description(&self) -> &str;

    /// JSON schema for the input parameters expected by the tool.
    fn input_schema(&self) -> Value;

    /// Execute the tool with the provided input arguments.
    /// Returns the tool result as a JSON Value or an error message string.
    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;
}

/// Registry storing executable tools by name.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new, empty ToolRegistry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Retrieve a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    /// Get all registered tools as a vector of Anthropic `Tool` structs.
    pub fn as_anthropic_tools(&self) -> Vec<anyllm_translate::anthropic::Tool> {
        self.tools
            .values()
            .map(|t| anyllm_translate::anthropic::Tool {
                name: t.name().to_string(),
                description: Some(t.description().to_string()),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Check if a tool is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Return all registered tool names.
    pub fn list_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
