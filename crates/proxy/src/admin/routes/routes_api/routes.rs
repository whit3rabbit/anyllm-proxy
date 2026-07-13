use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

use super::helpers::{
    audit, db_error_status, err_json, ok_json, route_to_response, CreateRouteRequest, RoutePatch,
    RouteRow,
};

pub(crate) async fn list_routes(State(shared): State<SharedState>) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, |conn| {
        let routes = crate::admin::db::list_routes(conn)?;
        let mut resp = Vec::with_capacity(routes.len());
        for r in &routes {
            let count = crate::admin::db::count_route_providers(conn, &r.id).unwrap_or(0);
            resp.push(route_to_response(r, count));
        }
        Ok::<_, rusqlite::Error>(resp)
    })
    .await;

    match result {
        Some(Ok(resp)) => ok_json(serde_json::json!({ "routes": resp })),
        _ => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list routes"),
    }
}

pub(crate) async fn create_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateRouteRequest>,
) -> axum::response::Response {
    if body.name.trim().is_empty() {
        return err_json(StatusCode::BAD_REQUEST, "name is required");
    }

    let now = crate::admin::db::now_iso8601();
    let row = RouteRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name.trim().to_string(),
        description: body.description,
        strategy: body.strategy,
        rpm: body.rpm,
        tpm: body.tpm,
        budget_usd: body.budget_usd,
        enabled: body.enabled,
        guardrail_mode: body.guardrail_mode,
        pxpipe_compress: body.pxpipe_compress,
        pxpipe_models: body.pxpipe_models,
        redact_secrets: body.redact_secrets,
        position: body.position,
        created_at: now.clone(),
        updated_at: now,
    };

    let row_clone = row.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::insert_route(conn, &row_clone)
    })
    .await;

    match result {
        Some(Ok(())) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_created",
                "route",
                row.name.clone(),
                None,
            );
            super::super::rebuild_route_router(&shared).await;
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(route_to_response(&row, 0)).unwrap()),
            )
                .into_response()
        }
        Some(Err(e)) => err_json(db_error_status(&e), e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create route"),
    }
}

pub(crate) async fn update_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RoutePatch>,
) -> axum::response::Response {
    let id_clone = id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        let updated = crate::admin::db::update_route(conn, &id_clone, &body)?;
        if !updated {
            return Ok::<_, rusqlite::Error>((false, None, 0));
        }
        let route = crate::admin::db::get_route(conn, &id_clone).ok().flatten();
        let count = crate::admin::db::count_route_providers(conn, &id_clone).unwrap_or(0);
        Ok((true, route, count))
    })
    .await;

    match result {
        Some(Ok((true, Some(r), count))) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_updated",
                "route",
                id,
                None,
            );
            super::super::rebuild_route_router(&shared).await;
            ok_json(route_to_response(&r, count))
        }
        Some(Ok((true, None, _))) => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "route not found after update",
        ),
        Some(Ok((false, _, _))) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update route"),
    }
}

pub(crate) async fn delete_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let id_clone = id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_route(conn, &id_clone)
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_deleted",
                "route",
                id,
                None,
            );
            super::super::rebuild_route_router(&shared).await;
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete route"),
    }
}
