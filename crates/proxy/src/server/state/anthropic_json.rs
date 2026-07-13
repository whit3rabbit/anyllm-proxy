use anyllm_translate::{anthropic, mapping};
use axum::{
    extract::{rejection::JsonRejection, FromRequest},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

/// Custom JSON extractor that returns Anthropic-shaped error responses on
/// parse failure. Axum's built-in Json returns its own error format, which
/// would break clients expecting Anthropic error shapes.
pub(crate) struct AnthropicJson<T>(pub T);

impl<S, T> FromRequest<S> for AnthropicJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AnthropicJson(value)),
            Err(rejection) => {
                let err = mapping::errors_map::create_anthropic_error(
                    anthropic::ErrorType::InvalidRequestError,
                    rejection.body_text(),
                    None,
                );
                Err((StatusCode::BAD_REQUEST, Json(err)).into_response())
            }
        }
    }
}
