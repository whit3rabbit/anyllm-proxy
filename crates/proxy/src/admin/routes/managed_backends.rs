// Re-export data types from db.rs. Route handlers will be added in a later task.
pub use crate::admin::db::{ManagedBackendPatch, ManagedBackendRow};

use crate::config::{
    BackendAuth, BackendConfig, BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig,
};
use anyllm_providers::provider::{AuthKind, ProviderDef, ProviderProtocol};

/// Convert a `ManagedBackendRow` + `ProviderDef` into a `BackendConfig` ready
/// to hand to `BackendClient::from_backend_config`.
///
/// Returns `None` for `ProviderProtocol::Custom` (not implemented).
pub fn row_to_backend_config(
    row: &ManagedBackendRow,
    provider: &ProviderDef,
) -> Option<BackendConfig> {
    let kind = match provider.protocol {
        ProviderProtocol::OpenAICompat => BackendKind::OpenAI,
        // GeminiOpenAI routes through the OpenAI client with a /openai path suffix.
        ProviderProtocol::GeminiOpenAI => BackendKind::OpenAI,
        ProviderProtocol::AzureOpenAI => BackendKind::AzureOpenAI,
        ProviderProtocol::VertexAI => BackendKind::Vertex,
        ProviderProtocol::GeminiNative => BackendKind::Gemini,
        ProviderProtocol::AnthropicNative => BackendKind::Anthropic,
        ProviderProtocol::BedrockNative => BackendKind::Bedrock,
        // Custom is explicitly unsupported; caller should log and skip.
        ProviderProtocol::Custom => return None,
    };

    let base_url = row
        .api_base
        .clone()
        .unwrap_or_else(|| provider.default_base_url.to_string());

    let api_key_str = row.api_key.clone().unwrap_or_default();

    let (backend_auth, bedrock_credentials) = match provider.auth {
        AuthKind::Bearer => (BackendAuth::BearerToken(api_key_str.clone()), None),
        AuthKind::GoogleApiKey => (BackendAuth::GoogleApiKey(api_key_str.clone()), None),
        AuthKind::AzureApiKey => (BackendAuth::AzureApiKey(api_key_str.clone()), None),
        AuthKind::AwsSigV4 => {
            // Bedrock: credentials come from row fields, not BackendAuth.
            let access_key = row.aws_access_key_id.clone().unwrap_or_default();
            let secret_key = row.aws_secret_access_key.clone().unwrap_or_default();
            let session_token = row.aws_session_token.clone();
            let creds = aws_credential_types::Credentials::new(
                access_key,
                secret_key,
                session_token,
                None,
                "managed_backend",
            );
            (BackendAuth::BearerToken(String::new()), Some(creds))
        }
        AuthKind::None => (BackendAuth::BearerToken(String::new()), None),
    };

    // api_key mirrors the inner token for non-Bedrock backends; empty for Bedrock.
    let api_key = match provider.auth {
        AuthKind::AwsSigV4 => String::new(),
        _ => api_key_str,
    };

    Some(BackendConfig {
        kind,
        api_key,
        base_url,
        api_format: OpenAIApiFormat::Chat,
        // Empty model mapping: managed backends don't apply Anthropic->backend name translation
        // by default. The caller (routing layer) will substitute the requested model name directly.
        model_mapping: ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: TlsConfig::default(),
        backend_auth,
        log_bodies: false,
        omit_stream_options: false,
        stream_timeout_secs: 0,
        bedrock_credentials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_providers::provider::{ProviderCapabilities, ProviderStatus};

    fn make_provider(protocol: ProviderProtocol, auth: AuthKind) -> ProviderDef {
        ProviderDef {
            id: "test",
            display_name: "Test",
            default_base_url: "https://api.test.com/v1",
            protocol,
            auth,
            status: ProviderStatus::Stub,
            env_vars: &[],
            litellm_prefix: "test/",
            capabilities: ProviderCapabilities::default(),
        }
    }

    fn make_row() -> ManagedBackendRow {
        ManagedBackendRow {
            id: "test-id".to_string(),
            name: "test".to_string(),
            provider_id: "test".to_string(),
            api_key: Some("sk-test".to_string()),
            api_base: None,
            deployment: None,
            api_version: None,
            project: None,
            region: None,
            aws_access_key_id: None,
            aws_secret_access_key: None,
            aws_session_token: None,
            rpm: None,
            tpm: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn openai_compat_maps_to_openai_kind() {
        let provider = make_provider(ProviderProtocol::OpenAICompat, AuthKind::Bearer);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::OpenAI);
        assert_eq!(bc.api_key, "sk-test");
        assert_eq!(bc.base_url, "https://api.test.com/v1");
    }

    #[test]
    fn gemini_openai_maps_to_openai_kind() {
        let provider = make_provider(ProviderProtocol::GeminiOpenAI, AuthKind::GoogleApiKey);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        // GeminiOpenAI routes through the OpenAI client.
        assert_eq!(bc.kind, BackendKind::OpenAI);
    }

    #[test]
    fn anthropic_native_maps_correctly() {
        let provider = make_provider(ProviderProtocol::AnthropicNative, AuthKind::Bearer);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Anthropic);
    }

    #[test]
    fn bedrock_builds_credentials() {
        let provider = make_provider(ProviderProtocol::BedrockNative, AuthKind::AwsSigV4);
        let mut row = make_row();
        row.api_key = None;
        row.aws_access_key_id = Some("AKIA123".to_string());
        row.aws_secret_access_key = Some("secret123".to_string());
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Bedrock);
        assert!(bc.bedrock_credentials.is_some());
        assert_eq!(bc.api_key, "");
    }

    #[test]
    fn custom_protocol_returns_none() {
        let provider = make_provider(ProviderProtocol::Custom, AuthKind::None);
        let row = make_row();
        assert!(row_to_backend_config(&row, &provider).is_none());
    }

    #[test]
    fn api_base_override_takes_priority() {
        let provider = make_provider(ProviderProtocol::OpenAICompat, AuthKind::Bearer);
        let mut row = make_row();
        row.api_base = Some("https://custom.endpoint.com/v1".to_string());
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.base_url, "https://custom.endpoint.com/v1");
    }

    #[test]
    fn default_base_url_used_when_api_base_absent() {
        let provider = make_provider(ProviderProtocol::OpenAICompat, AuthKind::Bearer);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.base_url, "https://api.test.com/v1");
    }

    #[test]
    fn azure_openai_maps_to_azure_openai_kind() {
        let provider = make_provider(ProviderProtocol::AzureOpenAI, AuthKind::AzureApiKey);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::AzureOpenAI);
    }

    #[test]
    fn vertex_ai_maps_to_vertex_kind() {
        let provider = make_provider(ProviderProtocol::VertexAI, AuthKind::Bearer);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Vertex);
    }

    #[test]
    fn gemini_native_maps_to_gemini_kind() {
        let provider = make_provider(ProviderProtocol::GeminiNative, AuthKind::GoogleApiKey);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Gemini);
    }

    #[test]
    fn google_api_key_auth_maps_correctly() {
        let provider = make_provider(ProviderProtocol::GeminiNative, AuthKind::GoogleApiKey);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        match bc.backend_auth {
            BackendAuth::GoogleApiKey(key) => {
                assert_eq!(key, "sk-test");
            }
            _ => panic!("Expected GoogleApiKey auth"),
        }
    }

    #[test]
    fn azure_api_key_auth_maps_correctly() {
        let provider = make_provider(ProviderProtocol::AzureOpenAI, AuthKind::AzureApiKey);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        match bc.backend_auth {
            BackendAuth::AzureApiKey(key) => {
                assert_eq!(key, "sk-test");
            }
            _ => panic!("Expected AzureApiKey auth"),
        }
    }

    #[test]
    fn none_auth_maps_to_empty_bearer_token() {
        let provider = make_provider(ProviderProtocol::OpenAICompat, AuthKind::None);
        let row = make_row();
        let bc = row_to_backend_config(&row, &provider).unwrap();
        match bc.backend_auth {
            BackendAuth::BearerToken(token) => {
                assert_eq!(token, "");
            }
            _ => panic!("Expected BearerToken auth"),
        }
    }
}
