use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

use super::helpers::{
    audit, build_provider_responses, db_error_status, err_json, ok_json, AddRouteProviderRequest,
    ReorderOutcome, ReorderRouteProvidersRequest, UpdateRouteProviderRequest,
};

pub(crate) async fn list_route_providers_handler(
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id)
            .ok()
            .flatten()
            .is_none()
        {
            return Ok::<_, rusqlite::Error>(None);
        }
        let providers = crate::admin::db::list_route_providers(conn, &route_id)?;
        let backends = crate::admin::db::list_managed_backends(conn).unwrap_or_default();
        Ok(Some(build_provider_responses(&providers, &backends)))
    })
    .await;

    match result {
        Some(Ok(Some(resp))) => ok_json(serde_json::json!({ "providers": resp })),
        Some(Ok(None)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        _ => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to list route providers",
        ),
    }
}

pub(crate) async fn add_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
    Json(body): Json<AddRouteProviderRequest>,
) -> axum::response::Response {
    let route_id_clone = route_id.clone();
    let backend_id = body.backend_id.clone();
    let models = body.models.clone();
    let priority = body.priority;
    let enabled = body.enabled;

    let backend_id_for_db = backend_id.clone();
    let backend_id_check = backend_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id_clone)
            .ok()
            .flatten()
            .is_none()
        {
            return Ok::<_, rusqlite::Error>(Err::<(), String>("route not found".into()));
        }
        if !crate::admin::db::managed_backend_exists(conn, &backend_id_check)? {
            return Ok(Err::<(), String>("backend not found".into()));
        }
        crate::admin::db::add_route_provider(
            conn,
            &route_id_clone,
            &backend_id_for_db,
            &models,
            priority,
            enabled,
        )?;
        Ok(Ok::<(), String>(()))
    })
    .await;

    match result {
        Some(Ok(Ok(()))) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_provider_added",
                "route_provider",
                route_id,
                Some(format!("backend_id={}", backend_id)),
            );
            super::super::rebuild_route_router(&shared).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Some(Ok(Err(msg))) => err_json(
            if msg == "route not found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            },
            msg,
        ),
        Some(Err(e)) => err_json(db_error_status(&e), e.to_string()),
        None => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to add route provider",
        ),
    }
}

pub(crate) async fn update_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path((route_id, provider_id)): Path<(String, String)>,
    Json(body): Json<UpdateRouteProviderRequest>,
) -> axum::response::Response {
    let provider_id_clone = provider_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::update_route_provider(
            conn,
            &provider_id_clone,
            body.models.as_deref(),
            body.priority,
            body.enabled,
        )
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_provider_updated",
                "route_provider",
                provider_id,
                Some(format!("route_id={}", route_id)),
            );
            super::super::rebuild_route_router(&shared).await;
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route provider not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update route provider",
        ),
    }
}

pub(crate) async fn reorder_route_providers_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
    Json(body): Json<ReorderRouteProvidersRequest>,
) -> axum::response::Response {
    let route_id_clone = route_id.clone();
    let ordered = body.provider_ids.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id_clone)
            .ok()
            .flatten()
            .is_none()
        {
            return Ok::<_, rusqlite::Error>(None);
        }
        let outcome = crate::admin::db::reorder_route_providers(conn, &route_id_clone, &ordered)?;
        match outcome {
            ReorderOutcome::Mismatch => Ok(Some(Err::<_, ()>(()))),
            ReorderOutcome::Ok(rows) => {
                let backends = crate::admin::db::list_managed_backends(conn).unwrap_or_default();
                Ok(Some(Ok(build_provider_responses(&rows, &backends))))
            }
        }
    })
    .await;

    match result {
        Some(Ok(Some(Ok(providers)))) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_providers_reordered",
                "route",
                route_id,
                Some(format!("count={}", providers.len())),
            );
            super::super::rebuild_route_router(&shared).await;
            ok_json(serde_json::json!({ "providers": providers }))
        }
        Some(Ok(Some(Err(())))) => err_json(
            StatusCode::BAD_REQUEST,
            "provider_ids must match the route's current provider set exactly",
        ),
        Some(Ok(None)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to reorder route providers",
        ),
    }
}

pub(crate) async fn remove_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path((_route_id, provider_id)): Path<(String, String)>,
) -> axum::response::Response {
    let provider_id_clone = provider_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::remove_route_provider(conn, &provider_id_clone)
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_provider_removed",
                "route_provider",
                provider_id,
                None,
            );
            super::super::rebuild_route_router(&shared).await;
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route provider not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to remove route provider",
        ),
    }
}
