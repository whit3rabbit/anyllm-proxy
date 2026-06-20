use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "groq",
    display_name: "Groq",
    default_base_url: "https://api.groq.com/openai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["GROQ_API_KEY"],
    litellm_prefix: "groq/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Production (GA) model catalog per https://console.groq.com/docs/models.
// Preview models (Llama 4 Scout/Maverick, Kimi K2, Qwen QwQ, DeepSeek R1 distill,
// Gemma2, Llama Guard) are intentionally excluded.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "llama-3.3-70b-versatile",
        provider_id: "groq",
        context_window: 131_072,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-8b-instant",
        provider_id: "groq",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai/gpt-oss-120b",
        provider_id: "groq",
        context_window: 131_072,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai/gpt-oss-20b",
        provider_id: "groq",
        context_window: 131_072,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Whisper speech-to-text. Non-chat endpoint; streaming/tool_use N/A.
    ModelDef {
        id: "whisper-large-v3",
        provider_id: "groq",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "whisper-large-v3-turbo",
        provider_id: "groq",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
