use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Replicate's native HTTP API is prediction-based (POST /v1/predictions with
// owner/model:version inputs), not OpenAI chat-compat. LiteLLM wraps it behind
// the `replicate/` prefix; we keep `OpenAICompat` + `Stub` so the existing
// registry contract is preserved until a real adapter lands.
// Docs: https://replicate.com/docs/reference/http
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "replicate",
    display_name: "Replicate",
    default_base_url: "https://api.replicate.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["REPLICATE_API_KEY", "REPLICATE_API_TOKEN"],
    litellm_prefix: "replicate/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        tool_choice: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// Representative popular GA models spanning text, image, and audio.
// Replicate model ids are `owner/model` (version pins are applied per-request,
// not encoded here). Context/output numbers reflect the underlying model card;
// image/audio entries use 0 where a chat-style context window does not apply.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama 3 70B Instruct — flagship open chat model.
    ModelDef {
        id: "meta/meta-llama-3-70b-instruct",
        provider_id: "replicate",
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
    // Meta Llama 3 8B Instruct — lightweight chat tier.
    ModelDef {
        id: "meta/meta-llama-3-8b-instruct",
        provider_id: "replicate",
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
    // Mistral 7B Instruct v0.2 — widely used small instruct model.
    ModelDef {
        id: "mistralai/mistral-7b-instruct-v0.2",
        provider_id: "replicate",
        context_window: 32_768,
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
    // Mixtral 8x7B Instruct — sparse MoE, strong reasoning for its class.
    ModelDef {
        id: "mistralai/mixtral-8x7b-instruct-v0.1",
        provider_id: "replicate",
        context_window: 32_768,
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
    // FLUX schnell — fast text-to-image, GA from Black Forest Labs.
    ModelDef {
        id: "black-forest-labs/flux-schnell",
        provider_id: "replicate",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // FLUX 1.1 Pro — higher-quality text-to-image tier.
    ModelDef {
        id: "black-forest-labs/flux-1.1-pro",
        provider_id: "replicate",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Stable Diffusion 3.5 Large — Stability AI flagship image model.
    ModelDef {
        id: "stability-ai/stable-diffusion-3.5-large",
        provider_id: "replicate",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // SDXL — long-running GA text-to-image baseline.
    ModelDef {
        id: "stability-ai/sdxl",
        provider_id: "replicate",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // OpenAI Whisper — speech-to-text. No chat context; audio in, text out.
    ModelDef {
        id: "openai/whisper",
        provider_id: "replicate",
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
    // Incredibly Fast Whisper — optimized ASR variant, popular on Replicate.
    ModelDef {
        id: "vaibhavs10/incredibly-fast-whisper",
        provider_id: "replicate",
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
