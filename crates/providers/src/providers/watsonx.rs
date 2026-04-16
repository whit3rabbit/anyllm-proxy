use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// IBM watsonx.ai — region-scoped foundation model API.
///
/// Auth: IBM Cloud IAM. An `apikey` is exchanged at `https://iam.cloud.ibm.com/identity/token`
/// for a short-lived `Bearer` token used against the regional `*.ml.cloud.ibm.com` host.
/// Requests must also carry a `project_id` (or `space_id`) in the JSON body.
///
/// Base URL is region-specific (us-south, eu-de, eu-gb, jp-tok, au-syd, ca-tor); we default to
/// Dallas (`us-south`) and let operators override via `WATSONX_URL` / `WATSONX_REGION`.
/// Endpoints used:
///   - `POST /ml/v1/text/chat?version=2024-05-01`        (chat completions)
///   - `POST /ml/v1/text/chat_stream?version=2024-05-01` (SSE streaming)
///   - `POST /ml/v1/text/generation?version=2023-05-02`  (legacy text generation)
///   - `POST /ml/v1/text/embeddings?version=2023-10-25`  (embeddings)
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "watsonx",
    display_name: "IBM watsonx.ai",
    default_base_url: "https://us-south.ml.cloud.ibm.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &[
        "WATSONX_API_KEY",
        "WATSONX_APIKEY",
        "WATSONX_PROJECT_ID",
        "WATSONX_URL",
        "WATSONX_REGION",
        "WATSONX_SPACE_ID",
        "WATSONX_TOKEN",
    ],
    litellm_prefix: "watsonx/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

/// Generally-available foundation models hosted by IBM on watsonx.ai multitenant
/// infrastructure. Context windows verified against LiteLLM
/// `model_prices_and_context_window.json` (sourced from IBM's official
/// `dataplatform.cloud.ibm.com/docs/.../fm-models.html` per BerriAI/litellm PR #15219).
///
/// Model availability is region-dependent on watsonx; not every model exists in every
/// data center. Deploy-on-demand and tech-preview models are omitted.
pub const MODELS: &[ModelDef] = &[
    // --- IBM Granite family ---
    ModelDef {
        id: "ibm/granite-3-3-8b-instruct",
        provider_id: "watsonx",
        context_window: 8_192,
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
        id: "ibm/granite-3-8b-instruct",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 1_024,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ibm/granite-4-h-small",
        provider_id: "watsonx",
        context_window: 20_480,
        max_output_tokens: 20_480,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ibm/granite-vision-3-2-2b",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ibm/granite-guardian-3-2-2b",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ibm/granite-guardian-3-3-8b",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Meta Llama family ---
    ModelDef {
        id: "meta-llama/llama-3-3-70b-instruct",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-3-2-1b-instruct",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-3-2-3b-instruct",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-3-2-11b-vision-instruct",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-3-2-90b-vision-instruct",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-4-maverick-17b",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/llama-guard-3-11b-vision",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Mistral family ---
    ModelDef {
        id: "mistralai/mistral-large",
        provider_id: "watsonx",
        context_window: 131_072,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistralai/mistral-medium-2505",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistralai/mistral-small-3-1-24b-instruct-2503",
        provider_id: "watsonx",
        context_window: 32_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistralai/pixtral-12b-2409",
        provider_id: "watsonx",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Other third-party ---
    ModelDef {
        id: "sdaia/allam-1-13b-instruct",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai/gpt-oss-120b",
        provider_id: "watsonx",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
