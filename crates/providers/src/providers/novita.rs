use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "novita",
    display_name: "Novita AI",
    // Official OpenAI-compatible endpoint per novita.ai/docs/guides/llm-api.
    // Clients append /v1/chat/completions, /v1/models, etc.
    default_base_url: "https://api.novita.ai/openai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["NOVITA_API_KEY"],
    litellm_prefix: "novita/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Function/tool calling is supported on many hosted models (Llama 3.x, Qwen, DeepSeek).
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        // Vision not documented as GA on novita OpenAI-compat path as of this writing.
        vision: false,
        batch: false,
    },
};

// Model IDs follow Novita's `vendor/model-name` convention.
// Context / output caps sourced from novita.ai/models/llm.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama family
    ModelDef {
        id: "meta-llama/llama-4-maverick-17b-128e-instruct-fp8",
        provider_id: "novita",
        context_window: 1_048_576,
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
        id: "meta-llama/llama-4-scout-17b-16e-instruct",
        provider_id: "novita",
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
        id: "meta-llama/llama-3.3-70b-instruct",
        provider_id: "novita",
        context_window: 131_072,
        max_output_tokens: 120_000,
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
        id: "meta-llama/llama-3.2-3b-instruct",
        provider_id: "novita",
        context_window: 32_768,
        max_output_tokens: 32_000,
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
        id: "meta-llama/llama-3.1-8b-instruct",
        provider_id: "novita",
        context_window: 16_384,
        max_output_tokens: 16_384,
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
        id: "meta-llama/llama-3-70b-instruct",
        provider_id: "novita",
        context_window: 8_192,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-3-8b-instruct",
        provider_id: "novita",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek family
    ModelDef {
        id: "deepseek/deepseek-v3.2",
        provider_id: "novita",
        context_window: 163_840,
        max_output_tokens: 65_536,
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
        id: "deepseek/deepseek-v3.1",
        provider_id: "novita",
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
        id: "deepseek/deepseek-r1-0528",
        provider_id: "novita",
        context_window: 163_840,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            // R1 is a reasoning model; exposes reasoning_content.
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek/deepseek-r1-distill-qwen-32b",
        provider_id: "novita",
        context_window: 64_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Qwen family
    ModelDef {
        id: "qwen/qwen3-8b",
        provider_id: "novita",
        context_window: 128_000,
        max_output_tokens: 20_000,
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
        id: "qwen/qwen2.5-72b-instruct",
        provider_id: "novita",
        context_window: 32_000,
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
        id: "qwen/qwen2.5-7b-instruct",
        provider_id: "novita",
        context_window: 32_000,
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
    // Other notable GA models
    ModelDef {
        id: "mistralai/mistral-nemo",
        provider_id: "novita",
        context_window: 60_288,
        max_output_tokens: 16_000,
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
        id: "google/gemma-3-27b-it",
        provider_id: "novita",
        context_window: 98_304,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "zhipu/glm-4.7-flash",
        provider_id: "novita",
        context_window: 200_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
