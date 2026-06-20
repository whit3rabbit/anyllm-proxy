use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// NLP Cloud uses per-model paths: /v1/<model>/<endpoint> and /v1/gpu/<model>/<endpoint>.
// Auth header is `Authorization: Token <key>` (not standard `Bearer`). `AuthKind::Bearer`
// is the closest match in the current enum; a dedicated `Token` variant would be more
// accurate if one is added later.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "nlp_cloud",
    display_name: "NLP Cloud",
    default_base_url: "https://api.nlpcloud.io/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["NLP_CLOUD_API_KEY"],
    litellm_prefix: "nlp_cloud/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        // NLP Cloud's /generation endpoint is request/response; long jobs use async
        // polling rather than SSE token streaming.
        streaming: false,
        tool_use: false,
        tool_choice: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

// Publicly GA generative + embeddings models on NLP Cloud. Context/output values come
// from docs.nlpcloud.com. Max-output for GPT-J / GPT-NeoX variants reflects the
// documented token caps on their respective hardware tiers (GPU where applicable).
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "chatdolphin",
        provider_id: "nlp_cloud",
        context_window: 8_192,
        max_output_tokens: 2_048,
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
        id: "dolphin",
        provider_id: "nlp_cloud",
        context_window: 8_192,
        max_output_tokens: 2_048,
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
        id: "dolphin-yi-34b",
        provider_id: "nlp_cloud",
        context_window: 8_192,
        max_output_tokens: 2_048,
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
        id: "dolphin-mixtral-8x7b",
        provider_id: "nlp_cloud",
        context_window: 32_768,
        max_output_tokens: 2_048,
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
        id: "finetuned-llama-3-70b",
        provider_id: "nlp_cloud",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "llama-3-1-405b",
        provider_id: "nlp_cloud",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "gpt-oss-120b",
        provider_id: "nlp_cloud",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "yi-34b",
        provider_id: "nlp_cloud",
        context_window: 4_096,
        max_output_tokens: 2_048,
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
        id: "mixtral-8x7b",
        provider_id: "nlp_cloud",
        context_window: 32_768,
        max_output_tokens: 2_048,
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
        id: "fast-gpt-j",
        provider_id: "nlp_cloud",
        context_window: 2_048,
        max_output_tokens: 2_048,
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
        id: "gpt-j",
        provider_id: "nlp_cloud",
        context_window: 2_048,
        max_output_tokens: 1_024,
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
        id: "finetuned-gpt-neox-20b",
        provider_id: "nlp_cloud",
        context_window: 2_048,
        max_output_tokens: 2_048,
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
        id: "gpt-neox-20b",
        provider_id: "nlp_cloud",
        context_window: 2_048,
        max_output_tokens: 1_024,
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
        id: "paraphrase-multilingual-mpnet-base-v2",
        provider_id: "nlp_cloud",
        context_window: 512,
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
