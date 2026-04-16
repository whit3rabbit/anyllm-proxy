use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Azure AI Foundry (Models-as-a-Service / Serverless API).
///
/// Covers the Azure AI Model Inference catalog (Cohere, Mistral, Meta Llama,
/// Microsoft Phi, DeepSeek, etc.) — distinct from Azure OpenAI. Endpoints are
/// per-deployment: either `https://<deployment>.<region>.models.ai.azure.com`
/// or `https://<resource>.services.ai.azure.com/models`. Set the base URL via
/// `AZURE_AI_API_BASE` or config.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "azure_ai",
    display_name: "Azure AI Foundry",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["AZURE_AI_API_KEY", "AZURE_AI_API_BASE"],
    litellm_prefix: "azure_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Model IDs match LiteLLM's `azure_ai/<id>` naming. Context windows and output
// caps come from the upstream model providers' public spec sheets; actual
// Azure deployment names are user-chosen, so callers typically override these.
pub const MODELS: &[ModelDef] = &[
    // Cohere
    ModelDef {
        id: "command-r-plus-08-2024",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-r-08-2024",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "cohere-embed-v3-english",
        provider_id: "azure_ai",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "cohere-embed-v3-multilingual",
        provider_id: "azure_ai",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Mistral
    ModelDef {
        id: "mistral-large-2407",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-small-2503",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-medium-2505",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ministral-3b",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "codestral-2501",
        provider_id: "azure_ai",
        context_window: 256_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Meta Llama
    ModelDef {
        id: "Meta-Llama-3.1-405B-Instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Meta-Llama-3.1-8B-Instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-3.3-70B-Instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-3.2-11B-Vision-Instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-3.2-90B-Vision-Instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Microsoft Phi
    ModelDef {
        id: "Phi-4",
        provider_id: "azure_ai",
        context_window: 16_384,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Phi-4-mini-instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Phi-4-multimodal-instruct",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek (hosted by Microsoft on Azure AI)
    ModelDef {
        id: "DeepSeek-V3-0324",
        provider_id: "azure_ai",
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
    ModelDef {
        id: "DeepSeek-R1",
        provider_id: "azure_ai",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
