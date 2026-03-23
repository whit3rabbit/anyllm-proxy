// Admin token validation middleware.
// All admin routes (except /admin/health) require Authorization: Bearer {token}.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// Axum middleware that validates the admin bearer token.
pub async fn validate_admin_token(
    token: axum::extract::State<Arc<String>>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let expected = format!("Bearer {}", token.as_str());

    match auth_header {
        Some(h) if h == expected => next.run(req).await,
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
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use axum::{body::Body, middleware, routing::get, Router};
    use tower::ServiceExt;

    fn test_app(token: &str) -> Router {
        let token = Arc::new(token.to_string());
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
