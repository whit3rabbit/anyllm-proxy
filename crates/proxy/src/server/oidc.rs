//! OIDC/JWT authentication support.
//!
//! When `OIDC_ISSUER_URL` is set, the proxy fetches the OpenID Connect
//! discovery document and JWKS at startup. Incoming Bearer tokens that
//! look like JWTs (contain two dots) are validated against the JWKS.
//! On validation failure, auth falls through to static/virtual key checks.

use crate::config::validate_base_url;
use anyllm_client::http::{build_http_client, HttpClientConfig};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Claims extracted from a validated JWT. Inserted into request extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub aud: Option<serde_json::Value>,
    pub exp: Option<u64>,
    pub iat: Option<u64>,
    /// Catch-all for custom claims.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// OIDC configuration loaded at startup from the discovery endpoint.
pub struct OidcConfig {
    pub audience: String,
    pub issuer: String,
    /// JWKS keys indexed by kid. Protected by RwLock for background refresh.
    keys: Arc<RwLock<Vec<JwkEntry>>>,
    jwks_uri: String,
    /// Reused for JWKS refresh calls.
    http_client: reqwest::Client,
}

struct JwkEntry {
    kid: String,
    algorithm: Algorithm,
    decoding_key: DecodingKey,
}

/// OpenID Connect discovery document (only fields we need).
#[derive(Deserialize)]
struct OidcDiscovery {
    issuer: String,
    jwks_uri: String,
}

/// JWKS response.
#[derive(Deserialize)]
struct JwksResponse {
    keys: Vec<JwkKey>,
}

/// Individual JWK (RSA, EC, and OKP/EdDSA supported).
#[derive(Deserialize)]
struct JwkKey {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    /// RSA modulus
    n: Option<String>,
    /// RSA exponent
    e: Option<String>,
    /// EC curve. Deserialized but unused; avoids serde unknown-field rejection.
    #[allow(dead_code)]
    crv: Option<String>,
    /// EC x coordinate
    x: Option<String>,
    /// EC y coordinate
    y: Option<String>,
}

/// Validate that an OIDC issuer URL or JWKS URI is safe to fetch.
/// Rejects private/loopback/metadata IP ranges to prevent SSRF.
pub fn validate_oidc_url(url: &str) -> Result<(), String> {
    validate_base_url(url)
}

impl OidcConfig {
    /// Discover OIDC configuration from the issuer URL.
    /// Fetches `.well-known/openid-configuration` and then the JWKS.
    /// Both the issuer URL and the discovered JWKS URI are validated against
    /// private/loopback/metadata IP ranges before any network call is made.
    pub async fn discover(issuer_url: &str, audience: &str) -> Result<Self, OidcError> {
        // Validate issuer URL before making any network call.
        validate_oidc_url(issuer_url)
            .map_err(|e| OidcError::Http(format!("OIDC issuer URL rejected (SSRF risk): {e}")))?;

        let client = build_http_client(&HttpClientConfig {
            ssrf_protection: true,
            connect_timeout: Some(std::time::Duration::from_secs(10)),
            ..Default::default()
        });

        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );
        let discovery: OidcDiscovery = client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| OidcError::Http(format!("OIDC discovery fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Http(format!("OIDC discovery parse failed: {e}")))?;

        // Validate the JWKS URI from the discovery document before fetching it.
        // A compromised or MITM'd discovery endpoint could redirect to an internal service.
        validate_oidc_url(&discovery.jwks_uri).map_err(|e| {
            OidcError::Http(format!(
                "JWKS URI in OIDC discovery document rejected (SSRF risk): {e}"
            ))
        })?;

        let config = Self {
            audience: audience.to_string(),
            issuer: discovery.issuer,
            keys: Arc::new(RwLock::new(Vec::new())),
            jwks_uri: discovery.jwks_uri,
            http_client: client,
        };

        config.refresh_jwks().await?;
        Ok(config)
    }

    /// Re-fetch the JWKS from the provider. Called periodically in the background.
    pub async fn refresh_jwks(&self) -> Result<(), OidcError> {
        let jwks: JwksResponse = self
            .http_client
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| OidcError::Http(format!("JWKS fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Http(format!("JWKS parse failed: {e}")))?;

        let mut entries = Vec::new();
        for key in &jwks.keys {
            if let Some(entry) = Self::parse_jwk(key) {
                entries.push(entry);
            }
        }

        if entries.is_empty() {
            return Err(OidcError::NoUsableKeys);
        }

        let mut guard = self.keys.write().unwrap_or_else(|e| e.into_inner());
        *guard = entries;
        Ok(())
    }

    fn parse_jwk(key: &JwkKey) -> Option<JwkEntry> {
        let kid = key.kid.clone().unwrap_or_default();
        let algorithm = match key.alg.as_deref() {
            Some("RS256") => Algorithm::RS256,
            Some("RS384") => Algorithm::RS384,
            Some("RS512") => Algorithm::RS512,
            Some("ES256") => Algorithm::ES256,
            Some("ES384") => Algorithm::ES384,
            Some("EdDSA") => Algorithm::EdDSA,
            // Default RSA keys without alg to RS256 (most common).
            None if key.kty == "RSA" => Algorithm::RS256,
            _ => return None,
        };

        let decoding_key = match key.kty.as_str() {
            "RSA" => {
                let n = key.n.as_ref()?;
                let e = key.e.as_ref()?;
                DecodingKey::from_rsa_components(n, e).ok()?
            }
            "EC" => {
                let x = key.x.as_ref()?;
                let y = key.y.as_ref()?;
                DecodingKey::from_ec_components(x, y).ok()?
            }
            "OKP" => {
                let x = key.x.as_ref()?;
                DecodingKey::from_ed_components(x).ok()?
            }
            _ => return None,
        };

        Some(JwkEntry {
            kid,
            algorithm,
            decoding_key,
        })
    }

    /// Validate a JWT token against the cached JWKS.
    /// Returns claims on success, error on failure.
    pub fn validate_token(&self, token: &str) -> Result<JwtClaims, OidcError> {
        let header =
            decode_header(token).map_err(|e| OidcError::Validation(format!("bad header: {e}")))?;

        let keys = self.keys.read().unwrap_or_else(|e| e.into_inner());

        // Find matching key by kid, or try all keys if no kid in header.
        let candidates: Vec<&JwkEntry> = if let Some(ref kid) = header.kid {
            keys.iter().filter(|k| k.kid == *kid).collect()
        } else {
            keys.iter().collect()
        };

        if candidates.is_empty() {
            return Err(OidcError::Validation(
                "no matching key found in JWKS".to_string(),
            ));
        }

        let mut validation = Validation::new(candidates[0].algorithm);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        // Accept reasonable clock skew (60 seconds).
        validation.leeway = 60;

        for candidate in &candidates {
            let mut v = validation.clone();
            v.algorithms = vec![candidate.algorithm];
            match decode::<JwtClaims>(token, &candidate.decoding_key, &v) {
                Ok(data) => return Ok(data.claims),
                Err(_) => continue,
            }
        }

        Err(OidcError::Validation(
            "token validation failed against all matching keys".to_string(),
        ))
    }
}

/// Check if a credential looks like a JWT (three Base64url segments separated by dots).
/// All characters in each segment must be `[A-Za-z0-9_-]` (Base64url alphabet).
/// This prevents API keys that happen to contain two dots from being sent through
/// JWT validation, which would add latency on every request.
pub fn looks_like_jwt(credential: &str) -> bool {
    if credential.len() <= 32 {
        return false;
    }
    let is_base64url = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    // splitn(4, '.') yields at most 4 parts: exactly 3 means two dots (valid JWT shape).
    let mut parts = credential.splitn(4, '.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => is_base64url(a) && is_base64url(b) && is_base64url(c),
        _ => false,
    }
}

#[derive(Debug)]
pub enum OidcError {
    Http(String),
    NoUsableKeys,
    Validation(String),
}

impl std::fmt::Display for OidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(msg) => write!(f, "OIDC HTTP error: {msg}"),
            Self::NoUsableKeys => write!(f, "OIDC: no usable keys in JWKS"),
            Self::Validation(msg) => write!(f, "JWT validation failed: {msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_oidc_url_rejects_private_ips() {
        // Cloud metadata endpoint.
        assert!(validate_oidc_url("http://169.254.169.254/oidc").is_err());
        // Loopback.
        assert!(validate_oidc_url("http://127.0.0.1/oidc").is_err());
        // RFC 1918 private range.
        assert!(validate_oidc_url("http://10.0.0.1/oidc").is_err());
        assert!(validate_oidc_url("http://192.168.1.1/oidc").is_err());
    }

    #[test]
    fn validate_oidc_url_accepts_public_https() {
        assert!(validate_oidc_url("https://accounts.google.com").is_ok());
        assert!(validate_oidc_url("https://login.microsoftonline.com/tenant/v2.0").is_ok());
    }

    #[test]
    fn looks_like_jwt_detects_jwt_shape() {
        // Typical JWT: header.payload.signature
        let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature_here";
        assert!(looks_like_jwt(jwt));
    }

    #[test]
    fn looks_like_jwt_rejects_api_keys() {
        assert!(!looks_like_jwt("sk-1234567890abcdef"));
        assert!(!looks_like_jwt("sk-vk-abcdef1234567890abcdef"));
        assert!(!looks_like_jwt("")); // empty
        assert!(!looks_like_jwt("a.b")); // only one dot
    }

    #[test]
    fn looks_like_jwt_rejects_short_dot_strings() {
        // Two dots but too short to be a real JWT.
        assert!(!looks_like_jwt("a.b.c"));
    }

    #[test]
    fn parse_rsa_jwk() {
        let key = JwkKey {
            kid: Some("test-kid".to_string()),
            kty: "RSA".to_string(),
            alg: Some("RS256".to_string()),
            // Valid base64url-encoded RSA components (minimal test values).
            n: Some("0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
        };
        let entry = OidcConfig::parse_jwk(&key);
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.kid, "test-kid");
        assert!(matches!(entry.algorithm, Algorithm::RS256));
    }

    #[test]
    fn parse_unknown_kty_returns_none() {
        let key = JwkKey {
            kid: Some("test".to_string()),
            kty: "oct".to_string(), // Symmetric keys, genuinely unsupported
            alg: Some("HS256".to_string()),
            n: None,
            e: None,
            crv: None,
            x: None,
            y: None,
        };
        assert!(OidcConfig::parse_jwk(&key).is_none());
    }

    #[test]
    fn parse_eddsa_jwk() {
        let key = JwkKey {
            kid: Some("ed-key".to_string()),
            kty: "OKP".to_string(),
            alg: Some("EdDSA".to_string()),
            n: None,
            e: None,
            crv: Some("Ed25519".to_string()),
            x: Some("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".to_string()),
            y: None,
        };
        let entry = OidcConfig::parse_jwk(&key);
        assert!(entry.is_some(), "EdDSA/OKP keys should be supported");
        let entry = entry.unwrap();
        assert_eq!(entry.kid, "ed-key");
        assert!(matches!(entry.algorithm, Algorithm::EdDSA));
    }
}

#[cfg(test)]
mod jwt_heuristic_tests {
    use super::looks_like_jwt;

    #[test]
    fn real_jwt_accepted() {
        // header.payload.signature — all Base64url chars
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1c2VyMSIsImlzcyI6Imh0dHBzOi8vaWRwIn0.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        assert!(looks_like_jwt(jwt));
    }

    #[test]
    fn api_key_with_dots_rejected() {
        // API key with two dots but non-Base64url chars (e.g. '+')
        assert!(!looks_like_jwt(
            "sk-abc+def.ghi+jkl.mno+pqr0123456789abcdef"
        ));
    }

    #[test]
    fn three_base64url_segments_required() {
        // Four dots — splitn(4, '.') produces 4 parts, not 3
        assert!(!looks_like_jwt(
            "eyJhbGci.eyJzdWIi.AAAAAAAA.extra0000000000000000000000000000000"
        ));
    }

    #[test]
    fn short_credential_rejected() {
        assert!(!looks_like_jwt("a.b.c"));
    }

    #[test]
    fn empty_segment_rejected() {
        assert!(!looks_like_jwt("abc..xyz012345678901234567890123456789"));
    }

    #[test]
    fn non_base64url_plus_slash_rejected() {
        // Base64 (not Base64url) chars '+' and '/'
        assert!(!looks_like_jwt(
            "abc+def.ghi/jkl.mno+pqr0000000000000000000"
        ));
    }
}
