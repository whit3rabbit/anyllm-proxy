use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// AWS SageMaker — SigV4-authenticated endpoint, no HTTP client implemented yet.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "sagemaker",
    display_name: "AWS SageMaker",
    default_base_url: "",
    protocol: ProviderProtocol::Custom,
    auth: AuthKind::AwsSigV4,
    status: ProviderStatus::Stub,
    env_vars: &[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_REGION_NAME",
    ],
    litellm_prefix: "sagemaker/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        tool_choice: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
