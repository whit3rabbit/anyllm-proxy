use crate::model::ModelDef;
use crate::provider::{ProviderDef, ProviderProtocol};
use crate::providers;
use std::collections::HashMap;
use std::sync::LazyLock;

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
    &providers::mlx_v::PROVIDER,
    &providers::nanogpt::PROVIDER,
    &providers::petals::PROVIDER,
    &providers::playht::PROVIDER,
    &providers::pollinations::PROVIDER,
    &providers::predibase::PROVIDER,
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
    ("mlx_v", providers::mlx_v::MODELS),
    ("nanogpt", providers::nanogpt::MODELS),
    ("petals", providers::petals::MODELS),
    ("playht", providers::playht::MODELS),
    ("pollinations", providers::pollinations::MODELS),
    ("predibase", providers::predibase::MODELS),
    ("siliconflow", providers::siliconflow::MODELS),
    ("triton", providers::triton::MODELS),
    ("hosted_vllm", providers::vllm::MODELS),
    ("xiaomi_mimo", providers::xiaomi_mimo::MODELS),
    ("xinference", providers::xinference::MODELS),
];

static PROVIDERS_BY_ID: LazyLock<HashMap<&'static str, &'static ProviderDef>> =
    LazyLock::new(|| {
        let mut map = HashMap::with_capacity(ALL_PROVIDERS.len() + LEGACY_ONLY_PROVIDERS.len());
        for provider in ALL_PROVIDERS
            .iter()
            .copied()
            .chain(LEGACY_ONLY_PROVIDERS.iter().copied())
        {
            map.insert(provider.id, provider);
        }
        map
    });

static PROVIDERS_BY_LITELLM_PREFIX: LazyLock<HashMap<&'static str, &'static ProviderDef>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        for provider in ALL_PROVIDERS
            .iter()
            .copied()
            .chain(LEGACY_ONLY_PROVIDERS.iter().copied())
        {
            if !provider.litellm_prefix.is_empty() {
                map.insert(provider.litellm_prefix, provider);
            }
        }
        map
    });

static MODELS_BY_PROVIDER: LazyLock<HashMap<&'static str, &'static [ModelDef]>> =
    LazyLock::new(|| {
        let mut map = HashMap::with_capacity(ALL_MODELS.len() + LEGACY_ONLY_MODELS.len());
        for (provider_id, models) in ALL_MODELS.iter().chain(LEGACY_ONLY_MODELS.iter()) {
            map.insert(*provider_id, *models);
        }
        map
    });

static MODEL_BY_PROVIDER_AND_ID: LazyLock<
    HashMap<(&'static str, &'static str), &'static ModelDef>,
> = LazyLock::new(|| {
    let model_count = ALL_MODELS
        .iter()
        .chain(LEGACY_ONLY_MODELS.iter())
        .map(|(_, models)| models.len())
        .sum();
    let mut map = HashMap::with_capacity(model_count);
    for (provider_id, models) in ALL_MODELS.iter().chain(LEGACY_ONLY_MODELS.iter()) {
        for model in *models {
            map.insert((*provider_id, model.id), model);
        }
    }
    map
});

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

/// Providers whose OpenAI-compatible endpoint rejects non-numeric or duplicate
/// tool-call IDs and therefore require outbound IDs to be rewritten to a safe
/// 9-digit sequential form (with the matching tool results re-paired).
///
/// This is a per-provider wire quirk, not a model capability, so it lives here
/// with the provider definitions rather than being hardcoded in the proxy.
pub const PROVIDERS_REQUIRING_NUMERIC_TOOL_CALL_IDS: &[&str] =
    &["mistral", "codestral", "openrouter"];

/// Whether outbound tool-call IDs must be rewritten to a safe numeric form for
/// this provider. See [`PROVIDERS_REQUIRING_NUMERIC_TOOL_CALL_IDS`].
pub fn requires_numeric_tool_call_ids(provider_id: &str) -> bool {
    let provider_id = canonical_provider_id(provider_id);
    PROVIDERS_REQUIRING_NUMERIC_TOOL_CALL_IDS.contains(&provider_id)
}

/// Look up a provider by its `id` field (e.g. `"groq"`, `"together_ai"`).
pub fn get_provider(id: &str) -> Option<&'static ProviderDef> {
    let id = canonical_provider_id(id);
    PROVIDERS_BY_ID.get(id).copied()
}

/// All registered providers.
pub fn all_providers() -> impl Iterator<Item = &'static ProviderDef> {
    ALL_PROVIDERS.iter().copied()
}

/// All models registered for a given provider id.
pub fn list_models(provider_id: &str) -> &'static [ModelDef] {
    let provider_id = canonical_provider_id(provider_id);
    MODELS_BY_PROVIDER.get(provider_id).copied().unwrap_or(&[])
}

/// Look up a specific model by provider id and model id.
pub fn get_model(provider_id: &str, model_id: &str) -> Option<&'static ModelDef> {
    let provider_id = canonical_provider_id(provider_id);
    MODEL_BY_PROVIDER_AND_ID
        .get(&(provider_id, model_id))
        .copied()
}

/// Whether a native Anthropic model supports LiteLLM's adaptive-thinking mode.
pub fn model_supports_anthropic_adaptive_thinking(provider_id: &str, model_id: &str) -> bool {
    canonical_provider_id(provider_id) == "anthropic"
        && providers::litellm_snapshot::ANTHROPIC_ADAPTIVE_THINKING_MODELS.contains(&model_id)
}

/// Whether a native Anthropic model has fully removed
/// `thinking: {"type": "enabled", "budget_tokens": N}` (400s on it), as
/// opposed to merely deprecating it (Opus 4.6 / Sonnet 4.6 still accept it).
pub fn model_requires_anthropic_adaptive_thinking(provider_id: &str, model_id: &str) -> bool {
    canonical_provider_id(provider_id) == "anthropic"
        && providers::litellm_snapshot::ANTHROPIC_ADAPTIVE_ONLY_THINKING_MODELS.contains(&model_id)
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
    if let Some(provider) = PROVIDERS_BY_LITELLM_PREFIX.get(prefix).copied() {
        return Some(provider);
    }

    let provider_id = prefix.strip_suffix('/')?;
    let canonical = canonical_provider_id(provider_id);
    // A bare provider id is only a valid routing prefix when it is an alias for a
    // different canonical id (e.g. "zhipuai/" -> "zai"). A non-alias bare id is not
    // a litellm_prefix, so it must not resolve here -- otherwise prefixes like
    // "baidu/" (baidu's real prefix is "qianfan/") would silently route to the
    // legacy provider instead of falling through to the OpenAI default.
    if canonical == provider_id {
        return None;
    }
    PROVIDERS_BY_ID.get(canonical).copied()
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
    fn tool_choice_capability_implies_tool_use() {
        let mut models_checked = 0;
        for provider in all_providers() {
            assert!(
                !provider.capabilities.tool_choice || provider.capabilities.tool_use,
                "provider '{}' advertises tool_choice without tool_use",
                provider.id
            );
            for model in list_models(provider.id) {
                models_checked += 1;
                assert!(
                    !model.capabilities.tool_choice || model.capabilities.tool_use,
                    "model '{}/{}' advertises tool_choice without tool_use",
                    provider.id,
                    model.id
                );
            }
        }
        assert!(models_checked > 0);
    }

    #[test]
    fn requires_numeric_tool_call_ids_matches_known_quirk_providers() {
        assert!(requires_numeric_tool_call_ids("mistral"));
        assert!(requires_numeric_tool_call_ids("codestral"));
        assert!(requires_numeric_tool_call_ids("openrouter"));
        assert!(!requires_numeric_tool_call_ids("openai"));
        assert!(!requires_numeric_tool_call_ids("groq"));
        // Every quirk provider id must resolve to a real provider so the list
        // cannot silently rot when a provider id is renamed/removed.
        for id in PROVIDERS_REQUIRING_NUMERIC_TOOL_CALL_IDS {
            assert!(
                get_provider(id).is_some(),
                "unknown quirk provider id '{id}'"
            );
        }
    }

    #[test]
    fn find_by_litellm_prefix_resolves_real_prefixes_and_aliases() {
        // Real litellm_prefix of a legacy provider resolves.
        assert_eq!(
            find_by_litellm_prefix("qianfan/").map(|p| p.id),
            Some("baidu")
        );
        // Alias bare id ("zhipuai" -> "zai") resolves to its canonical provider.
        assert_eq!(
            find_by_litellm_prefix("zhipuai/").map(|p| p.id),
            Some("zai")
        );
    }

    #[test]
    fn find_by_litellm_prefix_rejects_non_alias_bare_ids() {
        // baidu's real prefix is "qianfan/", iflytek's is "spark/". The bare id
        // prefix is not a routing prefix and must not resolve to the provider.
        assert!(find_by_litellm_prefix("baidu/").is_none());
        assert!(find_by_litellm_prefix("iflytek/").is_none());
    }

    #[test]
    fn anthropic_models_populated() {
        assert!(!list_models("anthropic").is_empty());
    }

    #[test]
    fn anthropic_models_match_litellm_snapshot() {
        let models = list_models("anthropic");
        assert_eq!(models.len(), 23);

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
    fn anthropic_adaptive_only_models_reject_budget_tokens_but_46_family_does_not() {
        assert!(model_requires_anthropic_adaptive_thinking(
            "anthropic",
            "claude-opus-4-8"
        ));
        assert!(model_requires_anthropic_adaptive_thinking(
            "anthropic",
            "claude-fable-5"
        ));
        // Opus 4.6 / Sonnet 4.6 support adaptive thinking but still accept
        // budget_tokens as a deprecated transitional escape hatch.
        assert!(!model_requires_anthropic_adaptive_thinking(
            "anthropic",
            "claude-opus-4-6"
        ));
        assert!(!model_requires_anthropic_adaptive_thinking(
            "anthropic",
            "claude-sonnet-4-6"
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
