use sha2::{Digest, Sha256};
use std::io::{self, Write};

/// Namespace prefix for cache keys, preventing cross-endpoint collisions.
#[derive(Debug, Clone, Copy)]
pub enum CacheNamespace {
    /// Anthropic /v1/messages endpoint.
    Anthropic,
    /// OpenAI /v1/chat/completions endpoint.
    OpenAI,
}

impl CacheNamespace {
    fn prefix(self) -> &'static str {
        match self {
            Self::Anthropic => "anth",
            Self::OpenAI => "oai",
        }
    }
}

pub struct CacheScope<'a> {
    pub backend_name: &'a str,
    pub auth_identity: &'a str,
    pub namespace: Option<&'a str>,
}

/// Compute a deterministic cache key for a request body.
///
/// Extracts the fields that affect response content, sorts them via BTreeMap,
/// serializes to canonical JSON, SHA-256 hashes the result, and prepends the
/// namespace prefix.
pub fn cache_key_for_request(
    body: &serde_json::Value,
    ns: CacheNamespace,
    scope: &CacheScope<'_>,
) -> String {
    let mut hasher = Sha256::new();
    write_canonical_cache_body(&mut hasher, body, scope);
    let hash = hasher.finalize();
    let hex = hex::encode(hash);
    format!("{}:{}", ns.prefix(), hex)
}

enum CacheField<'a> {
    Json(&'a str, &'a serde_json::Value),
    Str(&'static str, &'a str),
}

impl<'a> CacheField<'a> {
    fn key(&self) -> &str {
        match self {
            Self::Json(key, _) | Self::Str(key, _) => key,
        }
    }
}

struct HashWriter<'a> {
    hasher: &'a mut Sha256,
}

impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_canonical_cache_body(
    hasher: &mut Sha256,
    body: &serde_json::Value,
    scope: &CacheScope<'_>,
) {
    let mut fields = Vec::new();
    if let Some(obj) = body.as_object() {
        fields.extend(
            obj.iter()
                .filter(|(key, value)| should_include_cache_field(key, value))
                .map(|(key, value)| CacheField::Json(key.as_str(), value)),
        );
    }
    fields.push(CacheField::Str("_scope_auth", scope.auth_identity));
    fields.push(CacheField::Str("_scope_backend", scope.backend_name));
    if let Some(namespace) = scope.namespace {
        fields.push(CacheField::Str("_scope_cache_namespace", namespace));
    }
    fields.sort_unstable_by(|a, b| a.key().cmp(b.key()));

    let mut writer = HashWriter { hasher };
    writer
        .write_all(b"{")
        .expect("hash writer should not fail writing object start");
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            writer
                .write_all(b",")
                .expect("hash writer should not fail writing separator");
        }
        serde_json::to_writer(&mut writer, field.key())
            .expect("hash writer should not fail writing key");
        writer
            .write_all(b":")
            .expect("hash writer should not fail writing colon");
        match field {
            CacheField::Json(_, value) => serde_json::to_writer(&mut writer, value)
                .expect("hash writer should not fail writing JSON value"),
            CacheField::Str(_, value) => serde_json::to_writer(&mut writer, value)
                .expect("hash writer should not fail writing string value"),
        }
    }
    writer
        .write_all(b"}")
        .expect("hash writer should not fail writing object end");
}

fn should_include_cache_field(key: &str, value: &serde_json::Value) -> bool {
    if value.is_null() {
        return false;
    }
    // Exclude fields that do not affect the backend response:
    // - stream / stream_options: transport only (a cached non-stream response is
    //   replayed as a stream and vice versa).
    // - cache: request cache controls, handled outside response-content hashing.
    // - _scope_*: added separately as scope fields.
    // - user / metadata: tracking fields documented as "Ignored" (anyllm_translate
    //   ChatCompletionRequest user, anthropic Metadata). Hashing them fragments the
    //   cache per end-user with no correctness benefit (tenant isolation is already
    //   provided by _scope_auth).
    //
    // parallel_tool_calls is NOT excluded: backends that honor it (e.g. OpenAI)
    // produce different output for true vs false, so it must be part of the cache
    // identity. (Gemini/Vertex have it stripped before dispatch by the tool policy.)
    !matches!(
        key,
        "stream"
            | "stream_options"
            | "cache"
            | "_scope_auth"
            | "_scope_backend"
            | "_scope_cache_namespace"
            | "user"
            | "metadata"
    )
}
