use anyllm_translate::gemini::request::GenerateContentRequest;
use anyllm_translate::mapping::gemini_message_map;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

/// POST /v1beta/models/{model}:countTokens
///
/// Translates the Gemini request to Anthropic format and counts tokens using
/// the tiktoken o200k_base approximation. Returns `{"totalTokens": N}`.
/// No backend call is made — purely local computation.
pub(super) async fn gemini_count_tokens(
    model: &str,
    gemini_req: GenerateContentRequest,
) -> Response {
    let anthropic_req = gemini_message_map::gemini_to_anthropic_request(&gemini_req, model);
    match tokio::task::spawn_blocking(move || {
        crate::server::token_counting::count_request_tokens_sync(&anthropic_req)
    })
    .await
    {
        Ok(n) => Json(serde_json::json!({ "totalTokens": n })).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": {"code": 500, "message": "token counting failed", "status": "INTERNAL"}
            })),
        )
            .into_response(),
    }
}
