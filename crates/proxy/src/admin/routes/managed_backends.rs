// Struct definitions for managed (admin-persisted) backend configurations.
// Route handlers will be added in a later task.

/// A fully-hydrated backend row as stored in SQLite.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedBackendRow {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub project: Option<String>,
    pub region: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub aws_session_token: Option<String>,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Patch struct for partial updates — all fields optional.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ManagedBackendPatch {
    pub provider_id: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub project: Option<String>,
    pub region: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub aws_session_token: Option<String>,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
}
