use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "deepseek",
    display_name: "DeepSeek",
    default_base_url: "https://api.deepseek.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["DEEPSEEK_API_KEY"],
    litellm_prefix: "deepseek/",
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

pub const MODELS: &[ModelDef] = &[
    // deepseek-chat: non-thinking mode of DeepSeek-V3.2. Supports tool calls,
    // JSON output, and FIM completion (beta). 128K context; 8K max output.
    ModelDef {
        id: "deepseek-chat",
        provider_id: "deepseek",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // deepseek-reasoner: thinking mode of DeepSeek-V3.2. Supports JSON output
    // and chat prefix completion (beta). Does NOT support function calling or FIM.
    // 128K context; 64K max output (default 32K).
    ModelDef {
        id: "deepseek-reasoner",
        provider_id: "deepseek",
        context_window: 128_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
