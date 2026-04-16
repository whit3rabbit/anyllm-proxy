use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "vertex_ai",
    display_name: "Google Vertex AI",
    // Vertex constructs per-region URLs: https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/...
    // us-central1 is the most broadly supported region for Gemini; callers override via VERTEX_LOCATION.
    default_base_url: "https://us-central1-aiplatform.googleapis.com",
    protocol: ProviderProtocol::VertexAI,
    auth: AuthKind::GoogleApiKey,
    status: ProviderStatus::Implemented,
    // Canonical GCP auth uses a service-account JSON via GOOGLE_APPLICATION_CREDENTIALS.
    // VERTEX_PROJECT / VERTEX_LOCATION scope the endpoint; GOOGLE_CLOUD_PROJECT / GOOGLE_CLOUD_LOCATION
    // are the canonical gcloud names accepted as aliases. VERTEX_API_KEY / GOOGLE_ACCESS_TOKEN allow
    // bypassing ADC when a short-lived bearer or express-mode API key is supplied directly.
    env_vars: &[
        "GOOGLE_APPLICATION_CREDENTIALS",
        "VERTEX_PROJECT",
        "VERTEX_LOCATION",
        "GOOGLE_CLOUD_PROJECT",
        "GOOGLE_CLOUD_LOCATION",
        "VERTEX_API_KEY",
        "GOOGLE_ACCESS_TOKEN",
    ],
    litellm_prefix: "vertex_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: true,
    },
};

// Publicly GA Gemini models on Vertex AI. Context windows and max output tokens verified against
// https://docs.cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/<model>.
// Gemini 1.5 Pro / 1.5 Flash were fully retired by 2025-09-24 and are intentionally omitted.
pub const MODELS: &[ModelDef] = &[
    // Gemini 2.5 Pro - GA 2025-06-17. 1M input, 65,535 output. Reasoning-capable.
    ModelDef {
        id: "gemini-2.5-pro",
        provider_id: "vertex_ai",
        context_window: 1_048_576,
        max_output_tokens: 65_535,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Gemini 2.5 Flash - GA 2025-06-17. 1M input, 65,535 output. Reasoning-capable.
    ModelDef {
        id: "gemini-2.5-flash",
        provider_id: "vertex_ai",
        context_window: 1_048_576,
        max_output_tokens: 65_535,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Gemini 2.5 Flash-Lite - GA 2025-07-22. Balanced low-latency variant.
    ModelDef {
        id: "gemini-2.5-flash-lite",
        provider_id: "vertex_ai",
        context_window: 1_048_576,
        max_output_tokens: 65_535,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Gemini 2.0 Flash - GA 2025-02-05. 1M input, 8,192 output.
    // Note: as of 2026-03-06 restricted to existing customers; new projects should prefer 2.5 Flash.
    ModelDef {
        id: "gemini-2.0-flash",
        provider_id: "vertex_ai",
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
    // Gemini 2.0 Flash-Lite - GA 2025-02-25. Same restriction as 2.0 Flash from 2026-03-06.
    ModelDef {
        id: "gemini-2.0-flash-lite",
        provider_id: "vertex_ai",
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
];
