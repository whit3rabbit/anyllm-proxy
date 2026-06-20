use crate::model::ModelDef;
use crate::provider::{ProviderDef, ProviderProtocol};
use crate::providers;

/// All registered LiteLLM-compatible providers.
///
/// The compatibility snapshot is generated from LiteLLM's
/// `model_prices_and_context_window.json` by `scripts/check_litellm_providers.py`.
static ALL_PROVIDERS: &[&ProviderDef] = providers::litellm_snapshot::ALL_PROVIDERS;

/// Models keyed by LiteLLM provider id, in the same order as `ALL_PROVIDERS`.
static ALL_MODELS: &[(&str, &[ModelDef])] = providers::litellm_snapshot::ALL_MODELS;

/// Legacy anyllm provider ids accepted as aliases for their LiteLLM provider ids.
///
/// Do not add aliases here unless the target provider speaks the same upstream
/// API and model namespace closely enough to be a compatibility migration.
static PROVIDER_ALIASES: &[(&str, &str)] = &[
    ("ai_ml_api", "aiml"),
    ("exa", "exa_ai"),
    ("github", "github_copilot"),
    ("gmi_cloud", "gmi"),
    ("jina", "jina_ai"),
    ("public_ai", "publicai"),
    ("stability_ai", "stability"),
    ("zhipuai", "zai"),
];

/// Legacy local-only providers that are not in LiteLLM's pricing/model snapshot.
///
/// They stay resolvable for existing configs, but are intentionally omitted from
/// `all_providers()` so the primary advertised catalog remains LiteLLM-aligned.
static LEGACY_ONLY_PROVIDERS: &[&ProviderDef] = &[
    &providers::aleph_alpha::PROVIDER,
    &providers::baidu::PROVIDER,
    &providers::blackboxai::PROVIDER,
    &providers::brave::PROVIDER,
    &providers::bytez::PROVIDER,
    &providers::cartesia::PROVIDER,
    &providers::chutes::PROVIDER,
    &providers::clarifai::PROVIDER,
    &providers::docker_model_runner::PROVIDER,
    &providers::galadriel::PROVIDER,
    &providers::huggingface::PROVIDER,
    &providers::iflytek::PROVIDER,
    &providers::infinity::PROVIDER,
    &providers::llamafile::PROVIDER,
    &providers::lm_studio::PROVIDER,
    &providers::lmsys::PROVIDER,
    &providers::nanogpt::PROVIDER,
    &providers::petals::PROVIDER,
    &providers::playht::PROVIDER,
    &providers::pollinations::PROVIDER,
    &providers::predibase::PROVIDER,
    &providers::scaleway::PROVIDER,
    &providers::siliconflow::PROVIDER,
    &providers::triton::PROVIDER,
    &providers::vllm::PROVIDER,
    &providers::xiaomi_mimo::PROVIDER,
    &providers::xinference::PROVIDER,
];

static LEGACY_ONLY_MODELS: &[(&str, &[ModelDef])] = &[
    ("aleph_alpha", providers::aleph_alpha::MODELS),
    ("baidu", providers::baidu::MODELS),
    ("blackboxai", providers::blackboxai::MODELS),
    ("brave", providers::brave::MODELS),
    ("bytez", providers::bytez::MODELS),
    ("cartesia", providers::cartesia::MODELS),
    ("chutes", providers::chutes::MODELS),
    ("clarifai", providers::clarifai::MODELS),
    (
        "docker_model_runner",
        providers::docker_model_runner::MODELS,
    ),
    ("galadriel", providers::galadriel::MODELS),
    ("huggingface", providers::huggingface::MODELS),
    ("iflytek", providers::iflytek::MODELS),
    ("infinity", providers::infinity::MODELS),
    ("llamafile", providers::llamafile::MODELS),
    ("lm_studio", providers::lm_studio::MODELS),
    ("lmsys", providers::lmsys::MODELS),
    ("nanogpt", providers::nanogpt::MODELS),
    ("petals", providers::petals::MODELS),
    ("playht", providers::playht::MODELS),
    ("pollinations", providers::pollinations::MODELS),
    ("predibase", providers::predibase::MODELS),
    ("scaleway", providers::scaleway::MODELS),
    ("siliconflow", providers::siliconflow::MODELS),
    ("triton", providers::triton::MODELS),
    ("hosted_vllm", providers::vllm::MODELS),
    ("xiaomi_mimo", providers::xiaomi_mimo::MODELS),
    ("xinference", providers::xinference::MODELS),
];

#[cfg(feature = "runtime-catalog")]
pub(crate) fn advertised_provider_defs() -> &'static [&'static ProviderDef] {
    ALL_PROVIDERS
}

#[cfg(feature = "runtime-catalog")]
pub(crate) fn legacy_only_provider_defs() -> &'static [&'static ProviderDef] {
    LEGACY_ONLY_PROVIDERS
}

#[cfg(feature = "runtime-catalog")]
pub(crate) fn advertised_model_groups() -> &'static [(&'static str, &'static [ModelDef])] {
    ALL_MODELS
}

#[cfg(feature = "runtime-catalog")]
pub(crate) fn legacy_only_model_groups() -> &'static [(&'static str, &'static [ModelDef])] {
    LEGACY_ONLY_MODELS
}

/// Return the LiteLLM-canonical provider id for a local legacy provider id.
pub fn canonical_provider_id(id: &str) -> &str {
    PROVIDER_ALIASES
        .iter()
        .find(|(legacy, _)| *legacy == id)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(id)
}

/// Look up a provider by its `id` field (e.g. `"groq"`, `"together_ai"`).
pub fn get_provider(id: &str) -> Option<&'static ProviderDef> {
    let id = canonical_provider_id(id);
    ALL_PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .copied()
        .or_else(|| LEGACY_ONLY_PROVIDERS.iter().find(|p| p.id == id).copied())
}

/// All registered providers.
pub fn all_providers() -> impl Iterator<Item = &'static ProviderDef> {
    ALL_PROVIDERS.iter().copied()
}

/// All models registered for a given provider id.
pub fn list_models(provider_id: &str) -> &'static [ModelDef] {
    let provider_id = canonical_provider_id(provider_id);
    ALL_MODELS
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, models)| *models)
        .or_else(|| {
            LEGACY_ONLY_MODELS
                .iter()
                .find(|(id, _)| *id == provider_id)
                .map(|(_, models)| *models)
        })
        .unwrap_or(&[])
}

/// Look up a specific model by provider id and model id.
pub fn get_model(provider_id: &str, model_id: &str) -> Option<&'static ModelDef> {
    list_models(provider_id).iter().find(|m| m.id == model_id)
}

/// Whether a native Anthropic model supports LiteLLM's adaptive-thinking mode.
pub fn model_supports_anthropic_adaptive_thinking(provider_id: &str, model_id: &str) -> bool {
    canonical_provider_id(provider_id) == "anthropic"
        && providers::litellm_snapshot::ANTHROPIC_ADAPTIVE_THINKING_MODELS.contains(&model_id)
}

/// Whether a native Anthropic model supports a LiteLLM output-config effort.
pub fn model_supports_anthropic_reasoning_effort(
    provider_id: &str,
    model_id: &str,
    effort: &str,
) -> bool {
    if !model_supports_anthropic_adaptive_thinking(provider_id, model_id) {
        return false;
    }
    match effort {
        "minimal" | "low" | "medium" | "high" => true,
        "max" => {
            providers::litellm_snapshot::ANTHROPIC_MAX_REASONING_EFFORT_MODELS.contains(&model_id)
        }
        "xhigh" => {
            providers::litellm_snapshot::ANTHROPIC_XHIGH_REASONING_EFFORT_MODELS.contains(&model_id)
        }
        _ => false,
    }
}

/// Resolve a provider id to the BackendKind string and a default base URL override.
///
/// Returns `None` if the provider id is not recognized.
/// Returns `Some((backend_kind_str, base_url))` where:
/// - `backend_kind_str` matches the string accepted by `BackendKind` parsing in proxy config
///   (e.g. `"openai"`, `"anthropic"`, `"gemini"`, `"vertex"`, `"azure"`, `"bedrock"`)
/// - `base_url` is the provider's default base URL (may be empty for per-deployment providers)
pub fn resolve_backend(provider_id: &str) -> Option<(&'static str, &'static str)> {
    let p = get_provider(provider_id)?;
    let kind_str = match p.protocol {
        ProviderProtocol::OpenAICompat => "openai",
        ProviderProtocol::AzureOpenAI => "azure",
        ProviderProtocol::VertexAI => "vertex",
        ProviderProtocol::GeminiOpenAI => "gemini",
        ProviderProtocol::GeminiNative => "gemini",
        ProviderProtocol::AnthropicNative => "anthropic",
        ProviderProtocol::BedrockNative => "bedrock",
        ProviderProtocol::Custom => return None,
    };
    Some((kind_str, p.default_base_url))
}

/// Find a provider by its LiteLLM routing prefix (e.g. `"groq/"` or `"together_ai/"`).
/// Used by `parse_provider_model()` in litellm config parsing.
pub fn find_by_litellm_prefix(prefix: &str) -> Option<&'static ProviderDef> {
    let direct = ALL_PROVIDERS
        .iter()
        .find(|p| !p.litellm_prefix.is_empty() && prefix == p.litellm_prefix)
        .copied();
    if direct.is_some() {
        return direct;
    }

    let provider_id = prefix.strip_suffix('/')?;
    let canonical = canonical_provider_id(provider_id);
    if canonical == provider_id {
        return LEGACY_ONLY_PROVIDERS
            .iter()
            .find(|p| !p.litellm_prefix.is_empty() && prefix == p.litellm_prefix)
            .copied();
    }
    ALL_PROVIDERS.iter().find(|p| p.id == canonical).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_providers_resolve() {
        for provider in all_providers() {
            // Every provider with a non-Custom protocol must resolve to a backend kind.
            if provider.protocol != ProviderProtocol::Custom {
                let result = resolve_backend(provider.id);
                assert!(
                    result.is_some(),
                    "provider '{}' should resolve to a backend kind",
                    provider.id
                );
            }
        }
    }

    #[test]
    fn get_provider_lookup() {
        assert!(get_provider("openai").is_some());
        assert!(get_provider("groq").is_some());
        assert!(get_provider("nonexistent").is_none());
    }

    #[test]
    fn openai_models_populated() {
        assert!(!list_models("openai").is_empty());
    }

    #[test]
    fn anthropic_models_populated() {
        assert!(!list_models("anthropic").is_empty());
    }

    #[test]
    fn anthropic_models_match_litellm_snapshot() {
        let models = list_models("anthropic");
        assert_eq!(models.len(), 22);

        for id in [
            "claude-fable-5",
            "claude-opus-4-7",
            "claude-opus-4-7-20260416",
            "claude-opus-4-8",
            "claude-opus-4-6-20260205",
            "claude-haiku-4-5",
            "claude-opus-4-5",
            "claude-sonnet-4-5",
            "claude-opus-4-1",
            "claude-3-7-sonnet-20250219",
            "claude-3-opus-20240229",
        ] {
            assert!(get_model("anthropic", id).is_some(), "missing {id}");
        }

        let opus_47 = get_model("anthropic", "claude-opus-4-7").unwrap();
        assert_eq!(opus_47.context_window, 1_000_000);
        assert_eq!(opus_47.max_output_tokens, 128_000);

        let sonnet_40 = get_model("anthropic", "claude-sonnet-4-20250514").unwrap();
        assert_eq!(sonnet_40.context_window, 1_000_000);
        assert_eq!(sonnet_40.status, crate::model::ModelStatus::Deprecated);
    }

    #[test]
    fn anthropic_reasoning_support_tables_match_litellm_flags() {
        assert!(model_supports_anthropic_adaptive_thinking(
            "anthropic",
            "claude-opus-4-8"
        ));
        assert!(model_supports_anthropic_reasoning_effort(
            "anthropic",
            "claude-opus-4-8",
            "xhigh"
        ));
        assert!(model_supports_anthropic_reasoning_effort(
            "anthropic",
            "claude-sonnet-4-6",
            "max"
        ));
        assert!(!model_supports_anthropic_reasoning_effort(
            "anthropic",
            "claude-sonnet-4-6",
            "xhigh"
        ));
        assert!(!model_supports_anthropic_adaptive_thinking(
            "openai",
            "claude-opus-4-8"
        ));
    }

    #[test]
    fn litellm_snapshot_provider_names_are_canonical() {
        assert!(get_provider("gmi").is_some());
        assert!(get_provider("publicai").is_some());
        assert!(get_provider("zai").is_some());
        assert_eq!(get_provider("gmi_cloud").unwrap().id, "gmi");
        assert_eq!(get_provider("public_ai").unwrap().id, "publicai");
        assert_eq!(get_provider("zhipuai").unwrap().id, "zai");
        assert_eq!(list_models("zhipuai").len(), list_models("zai").len());
        assert_eq!(get_provider("lm_studio").unwrap().id, "lm_studio");
    }

    #[test]
    fn litellm_prefix_lookup() {
        let p = find_by_litellm_prefix("groq/").unwrap();
        assert_eq!(p.id, "groq");

        let p = find_by_litellm_prefix("together_ai/").unwrap();
        assert_eq!(p.id, "together_ai");

        let p = find_by_litellm_prefix("github_copilot/").unwrap();
        assert_eq!(p.id, "github_copilot");

        let p = find_by_litellm_prefix("gmi_cloud/").unwrap();
        assert_eq!(p.id, "gmi");

        let p = find_by_litellm_prefix("lm_studio/").unwrap();
        assert_eq!(p.id, "lm_studio");

        assert!(find_by_litellm_prefix("unknown/").is_none());
    }

    #[test]
    fn resolve_backend_stub_routes_to_openai() {
        let (kind, _url) = resolve_backend("groq").unwrap();
        assert_eq!(kind, "openai");
    }

    #[test]
    fn resolve_backend_anthropic() {
        let (kind, _) = resolve_backend("anthropic").unwrap();
        assert_eq!(kind, "anthropic");
    }
}
