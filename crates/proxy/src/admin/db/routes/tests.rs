use super::*;
use crate::admin::db::backends::ManagedBackendRow;

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    super::super::init_db(&conn).unwrap();
    conn
}

fn test_row(name: &str) -> ManagedBackendRow {
    ManagedBackendRow {
        id: format!("id-{name}"),
        name: name.to_string(),
        provider_id: "openai".to_string(),
        api_key: Some("sk-test".to_string()),
        api_base: None,
        deployment: None,
        api_version: None,
        project: None,
        region: None,
        aws_access_key_id: None,
        aws_secret_access_key: None,
        aws_session_token: None,
        rpm: Some(100),
        tpm: Some(10_000),
        enabled: true,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

fn seed_route_with_providers(conn: &Connection, count: usize) -> (String, Vec<String>) {
    let route = RouteRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: "r".into(),
        description: None,
        strategy: "failover".into(),
        rpm: None,
        tpm: None,
        budget_usd: None,
        enabled: true,
        guardrail_mode: None,
        pxpipe_compress: None,
        pxpipe_models: None,
        redact_secrets: None,
        position: 0,
        created_at: now_iso8601(),
        updated_at: now_iso8601(),
    };
    insert_route(conn, &route).unwrap();

    for i in 0..count {
        let mut b = test_row(&format!("b{i}"));
        b.id = format!("backend-{i}");
        crate::admin::db::backends::insert_managed_backend(conn, &b).unwrap();
        add_route_provider(conn, &route.id, &b.id, &["*".to_string()], i as i32, true).unwrap();
    }

    let ids: Vec<String> = list_route_providers(conn, &route.id)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    (route.id, ids)
}

#[test]
fn update_route_sets_and_clears_option_override() {
    let conn = in_memory_db();
    let (route_id, _) = seed_route_with_providers(&conn, 1);

    // Set an override.
    let set = RoutePatch {
        name: None,
        description: None,
        strategy: None,
        rpm: None,
        tpm: None,
        budget_usd: None,
        enabled: Some(false),
        guardrail_mode: Some(Some("standard".into())),
        pxpipe_compress: Some(Some(true)),
        pxpipe_models: None,
        redact_secrets: None,
        position: None,
    };
    assert!(update_route(&conn, &route_id, &set).unwrap());
    let r = get_route(&conn, &route_id).unwrap().unwrap();
    assert!(!r.enabled);
    assert_eq!(r.guardrail_mode.as_deref(), Some("standard"));
    assert_eq!(r.pxpipe_compress, Some(true));

    // Clear the override back to NULL (inherit).
    let clear = RoutePatch {
        name: None,
        description: None,
        strategy: None,
        rpm: None,
        tpm: None,
        budget_usd: None,
        enabled: None,
        guardrail_mode: Some(None),
        pxpipe_compress: Some(None),
        pxpipe_models: None,
        redact_secrets: None,
        position: None,
    };
    assert!(update_route(&conn, &route_id, &clear).unwrap());
    let r = get_route(&conn, &route_id).unwrap().unwrap();
    assert_eq!(r.guardrail_mode, None);
    assert_eq!(r.pxpipe_compress, None);
    // enabled was left unchanged (None) — still false.
    assert!(!r.enabled);
}

#[test]
fn disabled_route_excluded_from_enabled_route_ids() {
    let conn = in_memory_db();
    let (route_id, _) = seed_route_with_providers(&conn, 1);
    // The seeded backend is "b0"; while enabled the route id is returned.
    assert_eq!(
        enabled_route_ids_for_backend_name(&conn, "b0").unwrap(),
        vec![route_id.clone()]
    );

    // Disable the route -> it drops out of the virtual-key scope query.
    let patch = RoutePatch {
        name: None,
        description: None,
        strategy: None,
        rpm: None,
        tpm: None,
        budget_usd: None,
        enabled: Some(false),
        guardrail_mode: None,
        pxpipe_compress: None,
        pxpipe_models: None,
        redact_secrets: None,
        position: None,
    };
    assert!(update_route(&conn, &route_id, &patch).unwrap());
    assert!(enabled_route_ids_for_backend_name(&conn, "b0")
        .unwrap()
        .is_empty());
}

#[test]
fn reorder_route_providers_rewrites_priorities() {
    let conn = in_memory_db();
    let (route_id, ids) = seed_route_with_providers(&conn, 3);

    // Reverse the order.
    let reversed: Vec<String> = ids.iter().rev().cloned().collect();
    let outcome = reorder_route_providers(&conn, &route_id, &reversed).unwrap();

    match outcome {
        ReorderOutcome::Ok(rows) => {
            let new_order: Vec<String> = rows.iter().map(|p| p.id.clone()).collect();
            assert_eq!(new_order, reversed);
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(row.priority, i as i32);
            }
        }
        ReorderOutcome::Mismatch => panic!("expected Ok"),
    }
}

#[test]
fn reorder_route_providers_mismatch_rolls_back() {
    let conn = in_memory_db();
    let (route_id, ids) = seed_route_with_providers(&conn, 3);

    // Submit a subset — should be rejected as Mismatch, priorities unchanged.
    let partial: Vec<String> = ids.iter().take(2).cloned().collect();
    let outcome = reorder_route_providers(&conn, &route_id, &partial).unwrap();
    assert!(matches!(outcome, ReorderOutcome::Mismatch));

    // Priorities must be untouched.
    let rows = list_route_providers(&conn, &route_id).unwrap();
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            row.priority, i as i32,
            "priorities must be unchanged after Mismatch"
        );
    }
}

#[test]
fn reorder_route_providers_rejects_duplicates_and_extras() {
    let conn = in_memory_db();
    let (route_id, ids) = seed_route_with_providers(&conn, 3);

    // Duplicate id.
    let dup = vec![ids[0].clone(), ids[0].clone(), ids[1].clone()];
    assert!(matches!(
        reorder_route_providers(&conn, &route_id, &dup).unwrap(),
        ReorderOutcome::Mismatch
    ));

    // Extra id that isn't a provider on this route.
    let mut extra = ids.clone();
    extra.push("bogus-id".into());
    assert!(matches!(
        reorder_route_providers(&conn, &route_id, &extra).unwrap(),
        ReorderOutcome::Mismatch
    ));
}
