use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Petals — distributed inference over shared GPU clusters (local or swarm endpoint).
/// Self-hosted only: no public hosted REST API. Upstream ships a PyTorch/Transformers
/// client (`AutoDistributedModelForCausalLM`), not an OpenAI-compatible server; port
/// 31330 is the documented default for `petals.cli.run_server`. Users typically front
/// Petals with their own OpenAI-compat shim, hence the `OpenAICompat` protocol stub.
/// Models (BLOOM 176B, Llama 3.1 up to 405B, Mixtral 8x22B, Falcon 40B+) are swarm-dependent;
/// leaving MODELS empty since availability tracks volunteer GPUs at health.petals.dev.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "petals",
    display_name: "Petals",
    default_base_url: "http://localhost:31330",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "petals/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
