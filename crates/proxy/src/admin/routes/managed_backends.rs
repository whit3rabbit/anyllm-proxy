// Route handlers and data types for managed backend CRUD.
pub use crate::admin::db::{ManagedBackendPatch, ManagedBackendRow};

use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

/// Masked view of a managed backend — never includes raw credentials.
#[derive(serde::Serialize)]
struct ManagedBackendResponse {
    id: String,
    name: String,
    provider_id: String,
    /// true if api_key is Some and non-empty
    api_key_set: bool,
    /// true if both aws_access_key_id and aws_secret_access_key are set and non-empty
    aws_creds_set: bool,
    api_base: Option<String>,
    deployment: Option<String>,
    api_version: Option<String>,
    project: Option<String>,
    region: Option<String>,
    rpm: Option<u32>,
    tpm: Option<u64>,
    created_at: String,
    updated_at: String,
}

impl ManagedBackendResponse {
    fn from_row(row: &ManagedBackendRow) -> Self {
        let api_key_set = row
            .api_key
            .as_deref()
            .is_some_and(|k| !k.is_empty());
        let aws_creds_set = row
            .aws_access_key_id
            .as_deref()
            .is_some_and(|k| !k.is_empty())
            && row
                .aws_secret_access_key
                .as_deref()
                .is_some_and(|k| !k.is_empty());
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            provider_id: row.provider_id.clone(),
            api_key_set,
            aws_creds_set,
            api_base: row.api_base.clone(),
            deployment: row.deployment.clone(),
            api_version: row.api_version.clone(),
            project: row.project.clone(),
            region: row.region.clone(),
            rpm: row.rpm,
            tpm: row.tpm,
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

/// Request body for POST /admin/api/backends/managed.
#[derive(serde::Deserialize)]
pub(super) struct CreateManagedBackendRequest {
    name: String,
    provider_id: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    api_base: Option<String>,
    #[serde(default)]
    deployment: Option<String>,
    #[serde(default)]
    api_version: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    aws_access_key_id: Option<String>,
    #[serde(default)]
    aws_secret_access_key: Option<String>,
    #[serde(default)]
    aws_session_token: Option<String>,
    #[serde(default)]
    rpm: Option<u32>,
    #[serde(default)]
    tpm: Option<u64>,
}

/// GET /admin/api/backends/managed -- list all managed backends (masked).
pub(super) async fn list(State(shared): State<SharedState>) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::list_managed_backends(conn)
    })
    .await;

    match result {
        Some(Ok(rows)) => {
            let backends: Vec<ManagedBackendResponse> =
                rows.iter().map(ManagedBackendResponse::from_row).collect();
            Json(serde_json::json!({ "backends": backends })).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to list managed backends"})),
        )
            .into_response(),
    }
}

/// POST /admin/api/backends/managed -- create a new managed backend.
pub(super) async fn create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateManagedBackendRequest>,
) -> axum::response::Response {
    // 1. Validate name.
    if !super::is_safe_model_name(&body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid backend name"})),
        )
            .into_response();
    }

    // 2. Validate provider.
    let provider = match anyllm_providers::get_provider(&body.provider_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Unknown provider_id"})),
            )
                .into_response()
        }
    };

    // 3. Build the row.
    let now = crate::admin::db::now_iso8601();
    let row = ManagedBackendRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name.clone(),
        provider_id: body.provider_id.clone(),
        api_key: body.api_key.clone(),
        api_base: body.api_base.clone(),
        deployment: body.deployment.clone(),
        api_version: body.api_version.clone(),
        project: body.project.clone(),
        region: body.region.clone(),
        aws_access_key_id: body.aws_access_key_id.clone(),
        aws_secret_access_key: body.aws_secret_access_key.clone(),
        aws_session_token: body.aws_session_token.clone(),
        rpm: body.rpm,
        tpm: body.tpm,
        created_at: now.clone(),
        updated_at: now,
    };

    // 4. Build BackendConfig and BackendClient (validates protocol is supported).
    let backend_config = match row_to_backend_config(&row, provider) {
        Some(bc) => bc,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Provider uses unsupported protocol (Custom)"})),
            )
                .into_response()
        }
    };
    let backend_client = crate::backend::BackendClient::from_backend_config(&backend_config);

    // 5. Insert into SQLite — 409 on duplicate name.
    let row_clone = row.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::insert_managed_backend(conn, &row_clone)
    })
    .await;

    match db_result {
        Some(Ok(())) => {}
        Some(Err(e)) if e.to_string().contains("UNIQUE constraint failed") => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "A managed backend with that name already exists"})),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to create managed backend"})),
            )
                .into_response();
        }
    }

    // 6. Insert into in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.insert(row.name.clone(), (row.clone(), backend_client));
    }

    // 7. Emit audit.
    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_created".into(),
            target_type: "managed_backend".into(),
            target_id: Some(row.name.clone()),
            detail: Some(format!("provider_id={}", row.provider_id)),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    // 8. Return 201 with masked representation.
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "backend": ManagedBackendResponse::from_row(&row) })),
    )
        .into_response()
}

/// PUT /admin/api/backends/managed/{name} -- update a managed backend.
pub(super) async fn update(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
    Json(patch): Json<ManagedBackendPatch>,
) -> axum::response::Response {
    // 1. Look up existing row from in-memory map.
    let existing_row = {
        let map = shared
            .managed_backends
            .read()
            .unwrap_or_else(|e| e.into_inner());
        map.get(&name).map(|(row, _)| row.clone())
    };

    let existing_row = match existing_row {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response()
        }
    };

    // 2. Apply patch to produce updated row.
    let mut updated_row = existing_row.clone();
    if let Some(v) = patch.provider_id.clone() {
        updated_row.provider_id = v;
    }
    if let Some(v) = patch.api_key.clone() {
        updated_row.api_key = Some(v);
    }
    if let Some(v) = patch.api_base.clone() {
        updated_row.api_base = Some(v);
    }
    if let Some(v) = patch.deployment.clone() {
        updated_row.deployment = Some(v);
    }
    if let Some(v) = patch.api_version.clone() {
        updated_row.api_version = Some(v);
    }
    if let Some(v) = patch.project.clone() {
        updated_row.project = Some(v);
    }
    if let Some(v) = patch.region.clone() {
        updated_row.region = Some(v);
    }
    if let Some(v) = patch.aws_access_key_id.clone() {
        updated_row.aws_access_key_id = Some(v);
    }
    if let Some(v) = patch.aws_secret_access_key.clone() {
        updated_row.aws_secret_access_key = Some(v);
    }
    if let Some(v) = patch.aws_session_token.clone() {
        updated_row.aws_session_token = Some(v);
    }
    if let Some(v) = patch.rpm {
        updated_row.rpm = Some(v);
    }
    if let Some(v) = patch.tpm {
        updated_row.tpm = Some(v);
    }
    // updated_at reflects pre-update state; canonical value is in SQLite.
    // The db function independently generates the new timestamp.

    // 3. Build BackendConfig/Client from updated row.
    let provider = match anyllm_providers::get_provider(&updated_row.provider_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Unknown provider_id"})),
            )
                .into_response()
        }
    };

    let backend_config = match row_to_backend_config(&updated_row, provider) {
        Some(bc) => bc,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Provider uses unsupported protocol (Custom)"})),
            )
                .into_response()
        }
    };
    let new_client = crate::backend::BackendClient::from_backend_config(&backend_config);

    // 4. Update SQLite.
    let name_clone = name.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::update_managed_backend(conn, &name_clone, &patch)
    })
    .await;

    match db_result {
        Some(Ok(true)) => {}
        Some(Ok(false)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to update managed backend"})),
            )
                .into_response();
        }
    }

    // 5. Stamp updated_at before inserting into memory (SQLite already has the new value).
    // Sub-millisecond difference from SQLite's timestamp is acceptable; direction is correct.
    updated_row.updated_at = crate::admin::db::now_iso8601();

    // 6. Update in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.insert(name.clone(), (updated_row.clone(), new_client));
    }

    // 7. Emit audit.
    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_updated".into(),
            target_type: "managed_backend".into(),
            target_id: Some(name.clone()),
            detail: Some(format!("provider_id={}", updated_row.provider_id)),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({ "backend": ManagedBackendResponse::from_row(&updated_row) })),
    )
        .into_response()
}

/// DELETE /admin/api/backends/managed/{name} -- delete a managed backend.
pub(super) async fn delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    // 1. Delete from SQLite.
    let name_clone = name.clone();
    let db_result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_managed_backend(conn, &name_clone)
    })
    .await;

    match db_result {
        Some(Ok(true)) => {}
        Some(Ok(false)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Managed backend not found"})),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete managed backend"})),
            )
                .into_response();
        }
    }

    // 2. Remove from in-memory map.
    {
        let mut map = shared
            .managed_backends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.remove(&name);
    }

    // 3. Emit audit.
    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "managed_backend_deleted".into(),
            target_type: "managed_backend".into(),
            target_id: Some(name.clone()),
            detail: None,
            source_ip: Some(addr.ip().to_string()),
        },
    );

    Json(serde_json::json!({ "status": "deleted", "name": name })).into_response()
}

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

    // Bedrock: base_url is passed as the region string to BedrockClient::new, not a URL.
    // Vertex: construct the full endpoint URL from project+region if api_base is absent.
    let base_url = match provider.protocol {
        ProviderProtocol::BedrockNative => {
            row.region.clone().unwrap_or_else(|| "us-east-1".to_string())
        }
        ProviderProtocol::VertexAI => {
            // api_base must be the full Vertex endpoint URL if provided.
            // Otherwise construct from project+region; if neither is set the caller
            // will get an error at request time.
            //
            // Security: region and project are user-supplied but the constructed
            // hostname always ends with "-aiplatform.googleapis.com", so an attacker
            // cannot reach an arbitrary host via these two fields. The api_base
            // override (handled in the _ arm) is the only path to an arbitrary URL.
            row.api_base.clone().unwrap_or_else(|| {
                match (&row.project, &row.region) {
                    (Some(proj), Some(reg)) => format!(
                        "https://{reg}-aiplatform.googleapis.com/v1/projects/{proj}/locations/{reg}/endpoints/openapi"
                    ),
                    _ => provider.default_base_url.to_string(),
                }
            })
        }
        // Security: api_base is user-supplied (full host + protocol). SSRF is
        // mitigated at the HTTP client layer: the ssrf-protection Cargo feature
        // (enabled by default) installs a DNS resolver that rejects private/loopback
        // IPs (127.x, 10.x, 172.16-31.x, 192.168.x, 169.254.x) and disables
        // redirects. Access also requires admin Bearer token + localhost binding.
        _ => row
            .api_base
            .clone()
            .unwrap_or_else(|| provider.default_base_url.to_string()),
    };

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
        row.region = Some("us-west-2".to_string());
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Bedrock);
        assert!(bc.bedrock_credentials.is_some());
        assert_eq!(bc.api_key, "");
        // base_url is used as the region string for BedrockClient, not a URL.
        assert_eq!(bc.base_url, "us-west-2");
    }

    #[test]
    fn bedrock_defaults_region_to_us_east_1() {
        let provider = make_provider(ProviderProtocol::BedrockNative, AuthKind::AwsSigV4);
        let mut row = make_row();
        row.api_key = None;
        row.aws_access_key_id = Some("AKIA123".to_string());
        row.aws_secret_access_key = Some("secret123".to_string());
        // region is None — should fall back to "us-east-1"
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.base_url, "us-east-1");
    }

    #[test]
    fn vertex_constructs_url_from_project_and_region() {
        let provider = make_provider(ProviderProtocol::VertexAI, AuthKind::Bearer);
        let mut row = make_row();
        row.api_base = None;
        row.project = Some("my-gcp-project".to_string());
        row.region = Some("us-central1".to_string());
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.kind, BackendKind::Vertex);
        assert_eq!(
            bc.base_url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-gcp-project/locations/us-central1/endpoints/openapi"
        );
    }

    #[test]
    fn vertex_api_base_takes_priority_over_project_region() {
        let provider = make_provider(ProviderProtocol::VertexAI, AuthKind::Bearer);
        let mut row = make_row();
        row.api_base = Some("https://custom.vertex.example.com/v1".to_string());
        row.project = Some("my-gcp-project".to_string());
        row.region = Some("us-central1".to_string());
        let bc = row_to_backend_config(&row, &provider).unwrap();
        assert_eq!(bc.base_url, "https://custom.vertex.example.com/v1");
    }

    #[test]
    fn vertex_falls_back_to_default_when_no_base_no_project_region() {
        let provider = make_provider(ProviderProtocol::VertexAI, AuthKind::Bearer);
        let row = make_row(); // api_base=None, project=None, region=None
        let bc = row_to_backend_config(&row, &provider).unwrap();
        // Falls back to provider.default_base_url (empty string in real Vertex provider).
        assert_eq!(bc.base_url, "https://api.test.com/v1");
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
