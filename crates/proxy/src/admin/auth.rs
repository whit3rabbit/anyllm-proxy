// Admin token validation middleware.
// All admin routes (except /admin/health) require Authorization: Bearer {token}.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Constant-time string comparison to prevent timing side-channels.
pub(super) fn constant_time_eq(a: &str, b: &str) -> bool {
    // Length comparison leaks length info, but the Bearer prefix is fixed-length
    // and the token format (UUID) is fixed-length, so this is acceptable.
    a.len() == b.len() && a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Generate a cryptographically random CSRF token (32 bytes of entropy, hex-encoded, 64 chars).
/// Uses two UUID v4 values (each 122 bits of randomness) concatenated, giving ~244 bits.
pub fn generate_csrf_token() -> String {
    let a = uuid::Uuid::new_v4().as_simple().to_string();
    let b = uuid::Uuid::new_v4().as_simple().to_string();
    format!("{a}{b}")
}

/// Extract the csrf_token value from a Cookie header string.
pub fn extract_csrf_cookie(cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|pair| {
        let pair = pair.trim();
        pair.strip_prefix("csrf_token=").map(|v| v.to_string())
    })
}

/// Constant-time comparison of two CSRF tokens. Returns false for empty tokens.
pub fn validate_csrf_tokens(from_header: &str, from_cookie: &str) -> bool {
    if from_header.is_empty() || from_cookie.is_empty() {
        return false;
    }
    constant_time_eq(from_header, from_cookie)
}

/// Axum middleware that validates the admin bearer token.
pub async fn validate_admin_token(
    token: axum::extract::State<Arc<Zeroizing<String>>>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let expected = format!("Bearer {}", token.as_str());

    match auth_header {
        Some(h) if constant_time_eq(h, &expected) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": {
                    "type": "authentication_error",
                    "message": "Invalid or missing admin token"
                }
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod csrf_tests {
    use super::*;

    #[test]
    fn generate_csrf_token_has_correct_length() {
        let token = generate_csrf_token();
        // 32 random bytes hex-encoded = 64 chars
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn csrf_tokens_are_unique() {
        let a = generate_csrf_token();
        let b = generate_csrf_token();
        assert_ne!(a, b);
    }

    #[test]
    fn extract_csrf_cookie_finds_value() {
        let cookie_header = "csrf_token=abc123; session=xyz";
        assert_eq!(
            extract_csrf_cookie(cookie_header),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extract_csrf_cookie_returns_none_when_absent() {
        let cookie_header = "session=xyz";
        assert_eq!(extract_csrf_cookie(cookie_header), None);
    }

    #[test]
    fn validate_csrf_matching_tokens() {
        let token = "abc123def456abc123def456abc123def456abc123def456abc123def456abcd";
        assert!(validate_csrf_tokens(token, token));
    }

    #[test]
    fn validate_csrf_mismatched_tokens() {
        let a = "abc123def456abc123def456abc123def456abc123def456abc123def456abcd";
        let b = "000000def456abc123def456abc123def456abc123def456abc123def456abcd";
        assert!(!validate_csrf_tokens(a, b));
    }

    #[test]
    fn validate_csrf_empty_token_fails() {
        assert!(!validate_csrf_tokens("", ""));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use axum::{body::Body, middleware, routing::get, Router};
    use tower::ServiceExt;

    fn test_app(token: &str) -> Router {
        let token = Arc::new(Zeroizing::new(token.to_string()));
        Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                token.clone(),
                validate_admin_token,
            ))
            .with_state(token)
    }

    #[tokio::test]
    async fn valid_token_passes() {
        let app = test_app("test-token-123");
        let req = HttpRequest::builder()
            .uri("/protected")
            .header("authorization", "Bearer test-token-123")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_token_rejected() {
        let app = test_app("test-token-123");
        let req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_token_rejected() {
        let app = test_app("test-token-123");
        let req = HttpRequest::builder()
            .uri("/protected")
            .header("authorization", "Bearer wrong-token")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_prefix_required() {
        let app = test_app("test-token-123");
        let req = HttpRequest::builder()
            .uri("/protected")
            .header("authorization", "test-token-123")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
