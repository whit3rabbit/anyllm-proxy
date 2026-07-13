//! Optimizer ONNX-model admin API: detect presence + trigger an opt-in download.
//!
//! The model is never bundled or auto-downloaded. `GET .../model` reports whether the
//! proxy was built with the ONNX scorer and whether the verified artifact is on disk;
//! `POST .../model/download` starts a background download+sha256-verify. The proxy engine
//! loads the scorer lazily on its next request once the artifact appears, so the admin
//! server never needs a handle to the engine (see `crate::optimizer`).

use axum::{http::StatusCode, response::IntoResponse, Json};

/// `GET /admin/api/optimizer/model` — model tier status for the settings UI.
pub(super) async fn get_model_status() -> Json<crate::optimizer::ModelStatus> {
    Json(crate::optimizer::model_status())
}

/// `POST /admin/api/optimizer/model/download` — start a background download+verify.
/// 202 if started, 409 if one is already running, 400 if the ONNX tier isn't compiled in
/// or the pin is unresolved.
pub(super) async fn download_model() -> impl IntoResponse {
    if !crate::optimizer::ONNX_COMPILED_IN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "proxy was not built with the optimizer-onnx feature; \
                          rebuild with --features optimizer-onnx to use the ONNX scorer"
            })),
        )
            .into_response();
    }
    match crate::optimizer::begin_model_download() {
        Ok(cfg) => {
            // Download on the blocking pool so the ~170MB fetch never stalls the runtime.
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    crate::optimizer::run_model_download_blocking(&cfg)
                })
                .await;
            });
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({ "status": "downloading" })),
            )
                .into_response()
        }
        Err(message) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
    }
}
