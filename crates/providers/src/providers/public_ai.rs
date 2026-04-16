use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Public AI Inference Utility — nonprofit, sovereign-AI inference provider
// hosting publicly-funded open-weight models (Apertus from the Swiss AI
// Initiative, SEA-LION v4 from AI Singapore, Olmo-3 from AI2, EuroLLM from the
// UTTER project, DictaLM from DICTA). OpenAI-compatible API on vLLM backend.
// Docs: https://platform.publicai.co/docs
// Also exposed as an inference provider on Hugging Face (provider="publicai").
// litellm_prefix kept as "public_ai/" for catalog compatibility.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "public_ai",
    display_name: "PublicAI",
    default_base_url: "https://api.publicai.co/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["PUBLIC_AI_API_KEY"],
    litellm_prefix: "public_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Model IDs mirror the Hugging Face identifiers surfaced by
// https://huggingface.co/models?inference_provider=publicai, which is the
// canonical catalog Public AI publishes. Context windows reflect the
// upstream model cards; values are conservative where the provider has not
// published explicit deployment limits.
pub const MODELS: &[ModelDef] = &[
    // --- Swiss AI Initiative: Apertus (fully-open, reproducible) ---
    ModelDef {
        id: "swiss-ai/Apertus-8B-Instruct-2509",
        provider_id: "public_ai",
        context_window: 65_536,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "swiss-ai/Apertus-70B-Instruct-2509",
        provider_id: "public_ai",
        context_window: 65_536,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- AI Singapore: SEA-LION v4 (Southeast Asian languages) ---
    ModelDef {
        id: "aisingapore/Gemma-SEA-LION-v4-27B-IT",
        provider_id: "public_ai",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "aisingapore/Qwen-SEA-LION-v4-32B-IT",
        provider_id: "public_ai",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Allen Institute for AI: Olmo 3 (fully-open) ---
    ModelDef {
        id: "allenai/Olmo-3-7B-Instruct",
        provider_id: "public_ai",
        context_window: 65_536,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- DICTA (Israel): Hebrew-focused reasoning model ---
    ModelDef {
        id: "dicta-il/DictaLM-3.0-24B-Thinking",
        provider_id: "public_ai",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- UTTER project: EuroLLM (European multilingual) ---
    ModelDef {
        id: "utter-project/EuroLLM-22B-Instruct-2512",
        provider_id: "public_ai",
        context_window: 4_096,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
