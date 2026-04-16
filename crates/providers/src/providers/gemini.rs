use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "gemini",
    display_name: "Google AI Studio (Gemini)",
    default_base_url: "https://generativelanguage.googleapis.com/v1beta",
    protocol: ProviderProtocol::GeminiOpenAI,
    auth: AuthKind::GoogleApiKey,
    status: ProviderStatus::Implemented,
    env_vars: &["GEMINI_API_KEY"],
    litellm_prefix: "gemini/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "gemini-2.5-pro",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-2.5-flash",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-2.5-flash-lite",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-2.0-flash",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-2.0-flash-lite",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-1.5-pro",
        provider_id: "gemini",
        context_window: 2_097_152,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-1.5-flash",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-1.5-flash-8b",
        provider_id: "gemini",
        context_window: 1_048_576,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding model. max_output_tokens is not meaningful; set to 0 to match
    // the `text-embedding-3-*` convention in openai.rs.
    ModelDef {
        id: "gemini-embedding-001",
        provider_id: "gemini",
        context_window: 2_048,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
