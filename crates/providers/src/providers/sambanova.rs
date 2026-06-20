use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "sambanova",
    display_name: "SambaNova",
    default_base_url: "https://api.sambanova.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["SAMBANOVA_API_KEY"],
    litellm_prefix: "sambanova/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// SambaNova Cloud GA / production catalog. Sourced from
// https://sambanova-systems.mintlify.dev/docs/en/models/sambacloud-models
// and https://sambanova-systems.mintlify.dev/docs/en/features/function-calling.
// Context windows from the public models page; max_output_tokens is not
// published per-model, so we use conservative defaults (a fraction of the
// context window) rather than guess exact caps.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama
    ModelDef {
        id: "Meta-Llama-3.3-70B-Instruct",
        provider_id: "sambanova",
        context_window: 131_072,
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
    ModelDef {
        id: "Meta-Llama-3.1-8B-Instruct",
        provider_id: "sambanova",
        context_window: 16_384,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Llama 4 (vision + tool use).
    ModelDef {
        id: "Llama-4-Maverick-17B-128E-Instruct",
        provider_id: "sambanova",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek family.
    ModelDef {
        id: "DeepSeek-V3.1",
        provider_id: "sambanova",
        context_window: 131_072,
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
    ModelDef {
        id: "DeepSeek-R1-0528",
        provider_id: "sambanova",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Qwen.
    ModelDef {
        id: "Qwen3-32B",
        provider_id: "sambanova",
        context_window: 32_768,
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
    // OpenAI open-weights on SambaNova.
    ModelDef {
        id: "gpt-oss-120b",
        provider_id: "sambanova",
        context_window: 131_072,
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
    // MiniMax.
    ModelDef {
        id: "MiniMax-M2.5",
        provider_id: "sambanova",
        context_window: 163_840,
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
    // Embeddings.
    ModelDef {
        id: "E5-Mistral-7B-Instruct",
        provider_id: "sambanova",
        context_window: 4_096,
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
