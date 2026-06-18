use axum::{
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

const REDACTED_SECRET: &str = "[REDACTED_SECRET]";

#[derive(Debug)]
pub(crate) enum SecretRedactionError {
    #[cfg(not(feature = "secrets-scanner"))]
    FeatureDisabled,
    #[cfg(feature = "secrets-scanner")]
    ScannerUnavailable(String),
    SerializeFailed(String),
    DeserializeFailed(String),
    #[cfg(feature = "secrets-scanner")]
    InputTooLarge {
        size: usize,
        max: u64,
    },
    #[cfg(feature = "secrets-scanner")]
    ScanFailed(String),
    #[cfg(feature = "secrets-scanner")]
    JoinFailed(String),
}

impl std::fmt::Display for SecretRedactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(not(feature = "secrets-scanner"))]
            Self::FeatureDisabled => {
                write!(f, "secrets scanner feature is not compiled in")
            }
            Self::SerializeFailed(err) => write!(f, "request serialization failed: {err}"),
            Self::DeserializeFailed(err) => {
                write!(f, "redacted request deserialization failed: {err}")
            }
            #[cfg(feature = "secrets-scanner")]
            Self::ScannerUnavailable(err) => write!(f, "secrets scanner unavailable: {err}"),
            #[cfg(feature = "secrets-scanner")]
            Self::InputTooLarge { size, max } => {
                write!(
                    f,
                    "request body is too large to scan: {size} bytes exceeds {max}"
                )
            }
            #[cfg(feature = "secrets-scanner")]
            Self::ScanFailed(err) => write!(f, "secret redaction failed: {err}"),
            #[cfg(feature = "secrets-scanner")]
            Self::JoinFailed(err) => write!(f, "secret redaction task failed: {err}"),
        }
    }
}

impl std::error::Error for SecretRedactionError {}

pub(crate) fn ensure_available(enabled: bool) -> Result<(), String> {
    if !enabled {
        return Ok(());
    }

    #[cfg(feature = "secrets-scanner")]
    {
        // Force the LazyLock so a broken bundled ruleset surfaces here (at admin
        // enable time or startup) instead of as a 500 on every proxied request.
        match &*SCANNER {
            Ok(_) => Ok(()),
            Err(err) => Err(format!("secrets scanner failed to initialize: {err}")),
        }
    }

    #[cfg(not(feature = "secrets-scanner"))]
    {
        Err("REDACT_SECRETS requires the `secrets-scanner` feature".to_string())
    }
}

pub(crate) async fn redact_body(
    enabled: bool,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Bytes, SecretRedactionError> {
    let content_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok());
    redact_body_with_content_type(enabled, content_type, body).await
}

pub(crate) async fn redact_body_with_content_type(
    enabled: bool,
    content_type: Option<&str>,
    body: Bytes,
) -> Result<Bytes, SecretRedactionError> {
    if !enabled || body.is_empty() || !should_scan_content_type(content_type) {
        return Ok(body);
    }

    redact_bytes(body).await
}

pub(crate) async fn redact_json_value<T>(enabled: bool, value: T) -> Result<T, SecretRedactionError>
where
    T: Serialize + DeserializeOwned,
{
    if !enabled {
        return Ok(value);
    }

    let body = serde_json::to_vec(&value)
        .map(Bytes::from)
        .map_err(|err| SecretRedactionError::SerializeFailed(err.to_string()))?;
    let redacted = redact_bytes(body).await?;
    serde_json::from_slice(&redacted)
        .map_err(|err| SecretRedactionError::DeserializeFailed(err.to_string()))
}

pub(crate) fn error_response(error: SecretRedactionError) -> Response {
    tracing::warn!(%error, "request rejected during secret redaction");
    let status = error.status_code();
    let error_type = if status.is_client_error() {
        anyllm_translate::anthropic::ErrorType::InvalidRequestError
    } else {
        anyllm_translate::anthropic::ErrorType::ApiError
    };
    let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
        error_type,
        error.safe_message().to_string(),
        None,
    );
    (status, Json(err)).into_response()
}

impl SecretRedactionError {
    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            #[cfg(feature = "secrets-scanner")]
            Self::InputTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::DeserializeFailed(_) | Self::SerializeFailed(_) => StatusCode::BAD_REQUEST,
            #[cfg(not(feature = "secrets-scanner"))]
            Self::FeatureDisabled => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "secrets-scanner")]
            Self::ScannerUnavailable(_) | Self::ScanFailed(_) | Self::JoinFailed(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub(crate) fn safe_message(&self) -> &'static str {
        match self {
            #[cfg(feature = "secrets-scanner")]
            Self::InputTooLarge { .. } => "Request body is too large to scan for secrets.",
            Self::DeserializeFailed(_) | Self::SerializeFailed(_) => {
                "Request could not be scanned for secrets before forwarding."
            }
            #[cfg(not(feature = "secrets-scanner"))]
            Self::FeatureDisabled => "Request could not be scanned for secrets before forwarding.",
            #[cfg(feature = "secrets-scanner")]
            Self::ScannerUnavailable(_) | Self::ScanFailed(_) | Self::JoinFailed(_) => {
                "Request could not be scanned for secrets before forwarding."
            }
        }
    }
}

fn should_scan_content_type(content_type: Option<&str>) -> bool {
    // Fail closed: a missing or malformed Content-Type must not silently bypass
    // redaction, since callers can omit/mangle the header while the upstream
    // still parses the body as JSON. Only skip types we know are binary, where
    // substring redaction would corrupt the payload.
    let Some(content_type) = content_type else {
        return true;
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();

    if mime.is_empty() {
        return true;
    }

    const BINARY_PREFIXES: &[&str] = &["multipart/", "image/", "audio/", "video/"];
    if BINARY_PREFIXES.iter().any(|p| mime.starts_with(p)) {
        return false;
    }

    const BINARY_TYPES: &[&str] = &[
        "application/octet-stream",
        "application/pdf",
        "application/zip",
        "application/gzip",
        "application/x-protobuf",
    ];
    !BINARY_TYPES.contains(&mime.as_str())
}

#[cfg(feature = "secrets-scanner")]
async fn redact_bytes(body: Bytes) -> Result<Bytes, SecretRedactionError> {
    tokio::task::spawn_blocking(move || {
        let scanner = match &*SCANNER {
            Ok(scanner) => scanner,
            Err(err) => return Err(SecretRedactionError::ScannerUnavailable(err.clone())),
        };

        if let Ok(mut value) = serde_json::from_slice::<Value>(&body) {
            if body.len() as u64 > super::middleware::MAX_BODY_SIZE as u64 {
                return Err(SecretRedactionError::InputTooLarge {
                    size: body.len(),
                    max: super::middleware::MAX_BODY_SIZE as u64,
                });
            }
            redact_json_tree(&mut value, scanner)?;
            return serde_json::to_vec(&value)
                .map(Bytes::from)
                .map_err(|err| SecretRedactionError::SerializeFailed(err.to_string()));
        }

        scan_proxy_redacted(scanner, &body).map(Bytes::from)
    })
    .await
    .map_err(|err| SecretRedactionError::JoinFailed(err.to_string()))?
}

#[cfg(not(feature = "secrets-scanner"))]
async fn redact_bytes(_body: Bytes) -> Result<Bytes, SecretRedactionError> {
    Err(SecretRedactionError::FeatureDisabled)
}

#[cfg(feature = "secrets-scanner")]
static SCANNER: std::sync::LazyLock<Result<secrets_scanner::Scanner, String>> =
    std::sync::LazyLock::new(|| {
        let mut config = secrets_scanner::ScanConfig::proxy();
        config.max_file_size = super::middleware::MAX_BODY_SIZE as u64;
        secrets_scanner::Scanner::from_bundled()
            .map(|scanner| scanner.with_config(config))
            .map_err(|err| err.to_string())
    });

#[cfg(feature = "secrets-scanner")]
fn redact_json_tree(
    value: &mut Value,
    scanner: &secrets_scanner::Scanner,
) -> Result<(), SecretRedactionError> {
    redact_json_tree_with_key(value, None, scanner)
}

#[cfg(feature = "secrets-scanner")]
fn redact_json_tree_with_key(
    value: &mut Value,
    key: Option<&str>,
    scanner: &secrets_scanner::Scanner,
) -> Result<(), SecretRedactionError> {
    match value {
        Value::String(text) => {
            *text = redact_json_string(key, text, scanner)?;
        }
        Value::Array(items) => {
            for item in items {
                redact_json_tree_with_key(item, None, scanner)?;
            }
        }
        Value::Object(map) => {
            for (child_key, child_value) in map {
                redact_json_tree_with_key(child_value, Some(child_key), scanner)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

#[cfg(feature = "secrets-scanner")]
fn redact_json_string(
    key: Option<&str>,
    text: &str,
    scanner: &secrets_scanner::Scanner,
) -> Result<String, SecretRedactionError> {
    if text.is_empty() {
        return Ok(text.to_string());
    }

    let direct = scan_proxy_redacted(scanner, text.as_bytes())?;
    let direct = String::from_utf8(direct)
        .map_err(|err| SecretRedactionError::ScanFailed(err.to_string()))?;
    if direct != text {
        return Ok(direct);
    }

    let Some(key) = key else {
        return Ok(text.to_string());
    };

    let contextual = serde_json::to_vec(&serde_json::json!({ key: text }))
        .map_err(|err| SecretRedactionError::SerializeFailed(err.to_string()))?;
    let output = scanner
        .scan_proxy(&contextual)
        .map_err(secret_error_from_proxy)?;

    if output.has_findings() {
        Ok(REDACTED_SECRET.to_string())
    } else {
        Ok(text.to_string())
    }
}

#[cfg(feature = "secrets-scanner")]
fn scan_proxy_redacted(
    scanner: &secrets_scanner::Scanner,
    body: &[u8],
) -> Result<Vec<u8>, SecretRedactionError> {
    scanner
        .scan_proxy(body)
        .map(|output| output.redacted)
        .map_err(secret_error_from_proxy)
}

#[cfg(feature = "secrets-scanner")]
fn secret_error_from_proxy(error: secrets_scanner::ProxyError) -> SecretRedactionError {
    match error {
        secrets_scanner::ProxyError::InputTooLarge { size, max } => {
            SecretRedactionError::InputTooLarge { size, max }
        }
        other => SecretRedactionError::ScanFailed(other.to_string()),
    }
}
