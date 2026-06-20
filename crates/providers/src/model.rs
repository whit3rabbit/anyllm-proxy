/// Features a specific model supports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub tool_use: bool,
    pub tool_choice: bool,
    pub vision: bool,
    /// Extended thinking / reasoning tokens (e.g. Claude extended thinking, o1/o3).
    pub extended_thinking: bool,
}

/// Availability of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModelStatus {
    Available,
    Deprecated,
    /// Listed for future completeness; not verified.
    Stub,
}

/// Metadata for a specific model. No pricing data; that lives in the model pricing JSON assets.
///
/// All fields are `'static` so model definitions can be compile-time constants.
#[derive(Debug, Clone)]
pub struct ModelDef {
    /// Model identifier as used in API requests (no provider prefix).
    pub id: &'static str,
    /// Provider this model belongs to (matches `ProviderDef.id`).
    pub provider_id: &'static str,
    /// Maximum input context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens per request.
    pub max_output_tokens: u32,
    /// Feature support for this specific model.
    pub capabilities: ModelCapabilities,
    /// Availability status.
    pub status: ModelStatus,
}
