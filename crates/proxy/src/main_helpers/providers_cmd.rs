use std::sync::LazyLock;

/// Shared HTTP client for provider model discovery (connect 10s, read 20s, no redirects).
pub static PROVIDER_REFRESH_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to build provider refresh HTTP client")
});

pub fn provider_status_str(s: anyllm_providers::provider::ProviderStatus) -> &'static str {
    match s {
        anyllm_providers::provider::ProviderStatus::Implemented => "implemented",
        anyllm_providers::provider::ProviderStatus::Wired => "wired",
        anyllm_providers::provider::ProviderStatus::Stub => "stub",
    }
}

pub fn provider_protocol_str(p: anyllm_providers::provider::ProviderProtocol) -> &'static str {
    match p {
        anyllm_providers::provider::ProviderProtocol::OpenAICompat => "openai_compat",
        anyllm_providers::provider::ProviderProtocol::AzureOpenAI => "azure_openai",
        anyllm_providers::provider::ProviderProtocol::VertexAI => "vertex_ai",
        anyllm_providers::provider::ProviderProtocol::GeminiOpenAI => "gemini_openai",
        anyllm_providers::provider::ProviderProtocol::GeminiNative => "gemini_native",
        anyllm_providers::provider::ProviderProtocol::AnthropicNative => "anthropic_native",
        anyllm_providers::provider::ProviderProtocol::BedrockNative => "bedrock_native",
        anyllm_providers::provider::ProviderProtocol::Custom => "custom",
    }
}

/// CLI handler for `anyllm-proxy providers …`.
/// Runs synchronously (blocking HTTP calls via a throw-away tokio runtime).
/// Does not write to SQLite — the HTTP fetch is informational only.
pub fn providers_subcommand(args: Vec<String>, _data_dir: &std::path::Path) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            let json_mode = args.iter().any(|a| a == "--json");
            let providers: Vec<_> = anyllm_providers::all_providers().collect();
            if json_mode {
                let out: Vec<serde_json::Value> = providers
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "id":               p.id,
                            "display_name":     p.display_name,
                            "status":           provider_status_str(p.status),
                            "protocol":         provider_protocol_str(p.protocol),
                            "chat_completions": p.capabilities.chat_completions,
                            "model_count":      anyllm_providers::list_models(p.id).len(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!(
                    "{:<20} {:<15} {:<12} {:>8}",
                    "ID", "STATUS", "PROTOCOL", "MODELS"
                );
                println!("{}", "-".repeat(60));
                for p in &providers {
                    println!(
                        "{:<20} {:<15} {:<12} {:>8}",
                        p.id,
                        provider_status_str(p.status),
                        provider_protocol_str(p.protocol),
                        anyllm_providers::list_models(p.id).len()
                    );
                }
            }
            0
        }
        Some("refresh") => {
            let target = args.get(1).map(String::as_str);
            let refresh_all = target == Some("--all");

            // Collect providers to refresh before entering async context.
            let providers_to_refresh: Vec<anyllm_providers::provider::ProviderDef> = if refresh_all
            {
                anyllm_providers::all_providers()
                    .filter(|p| p.capabilities.chat_completions)
                    .filter(|p| p.env_vars.iter().any(|v| std::env::var(v).is_ok()))
                    .cloned()
                    .collect()
            } else if let Some(id) = target {
                match anyllm_providers::get_provider(id) {
                    Some(p) => vec![p.clone()],
                    None => {
                        eprintln!("error: unknown provider '{id}'");
                        return 1;
                    }
                }
            } else {
                eprintln!("usage: anyllm-proxy providers refresh <provider-id>|--all");
                return 1;
            };

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime");

            let client = PROVIDER_REFRESH_CLIENT.clone();

            let mut exit = 0;
            for provider in &providers_to_refresh {
                let api_key = provider.env_vars.iter().find_map(|v| std::env::var(v).ok());
                let url = format!(
                    "{}/v1/models",
                    provider.default_base_url.trim_end_matches('/')
                );
                let result = rt.block_on(async {
                    let mut req = client.get(&url);
                    if let Some(ref key) = api_key {
                        req = req.header("Authorization", format!("Bearer {key}"));
                    }
                    req.send().await
                });
                match result {
                    Err(e) => {
                        eprintln!("{}: error: {e}", provider.id);
                        exit = 1;
                    }
                    Ok(resp) if !resp.status().is_success() => {
                        eprintln!("{}: upstream returned {}", provider.id, resp.status());
                        exit = 1;
                    }
                    Ok(resp) => match rt.block_on(resp.json::<serde_json::Value>()) {
                        Err(e) => {
                            eprintln!("{}: invalid JSON: {e}", provider.id);
                            exit = 1;
                        }
                        Ok(json) => {
                            let models: Vec<&str> = json
                                .get("data")
                                .and_then(|d| d.as_array())
                                .map(|arr| {
                                    arr.iter().filter_map(|m| m.get("id")?.as_str()).collect()
                                })
                                .unwrap_or_default();
                            println!("{}: {} models", provider.id, models.len());
                            for m in &models {
                                println!("  - {m}");
                            }
                        }
                    },
                }
            }
            exit
        }
        _ => {
            eprintln!("usage: anyllm-proxy providers list [--json]");
            eprintln!("       anyllm-proxy providers refresh <provider-id>");
            eprintln!("       anyllm-proxy providers refresh --all");
            1
        }
    }
}
