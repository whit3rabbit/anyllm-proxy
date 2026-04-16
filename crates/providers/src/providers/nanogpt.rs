use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// NanoGPT is a pay-as-you-go aggregator that fronts 700+ models from OpenAI,
// Anthropic, Google, Meta, DeepSeek, Qwen, Mistral and others behind a single
// OpenAI-compatible endpoint at https://nano-gpt.com/api/v1/chat/completions.
// Auth is a Bearer token (also accepts `x-api-key`). The catalog below is a
// representative slice of popular GA models; the full list is discoverable at
// GET /api/v1/models.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "nanogpt",
    display_name: "NanoGPT",
    default_base_url: "https://nano-gpt.com/api/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["NANOGPT_API_KEY"],
    litellm_prefix: "nanogpt/",
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
    // OpenAI (proxied via NanoGPT)
    ModelDef {
        id: "gpt-4o",
        provider_id: "nanogpt",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4o-mini",
        provider_id: "nanogpt",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "o1",
        provider_id: "nanogpt",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Anthropic (proxied)
    ModelDef {
        id: "claude-3-5-sonnet-20241022",
        provider_id: "nanogpt",
        context_window: 200_000,
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
        id: "claude-3-5-haiku-20241022",
        provider_id: "nanogpt",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Google Gemini (proxied)
    ModelDef {
        id: "gemini-2.5-pro",
        provider_id: "nanogpt",
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
        provider_id: "nanogpt",
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
    // Meta Llama
    ModelDef {
        id: "meta-llama/llama-3.3-70b-instruct",
        provider_id: "nanogpt",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek
    ModelDef {
        id: "deepseek-v3",
        provider_id: "nanogpt",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Qwen
    ModelDef {
        id: "qwen3-235b-a22b",
        provider_id: "nanogpt",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
