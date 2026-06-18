use super::*;

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    super::super::init_db(&conn).unwrap();
    conn
}

fn sample_entry() -> RequestLogEntry {
    RequestLogEntry {
        request_id: "test-123".into(),
        timestamp: "2099-01-01T00:00:00Z".into(),
        backend: "openai".into(),
        model_requested: Some("claude-sonnet-4-6".into()),
        model_mapped: Some("gpt-4o".into()),
        status_code: 200,
        latency_ms: 342,
        input_tokens: Some(150),
        output_tokens: Some(87),
        is_streaming: false,
        error_message: None,
        error_kind: None,
        key_id: None,
        cost_usd: None,
    }
}

#[test]
fn insert_and_query_request_log() {
    let conn = in_memory_db();
    let entry = sample_entry();
    insert_request_log(&conn, &entry).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].request_id, "test-123");
    assert_eq!(results[0].status_code, 200);
    assert_eq!(results[0].latency_ms, 342);
    assert_eq!(results[0].input_tokens, Some(150));
}

#[test]
fn query_with_backend_filter() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let mut entry2 = sample_entry();
    entry2.request_id = "test-456".into();
    entry2.backend = "gemini".into();
    insert_request_log(&conn, &entry2).unwrap();

    let results = query_request_log(&conn, 10, 0, Some("gemini"), None, None, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend, "gemini");
}

#[test]
fn query_with_status_filter() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let mut err_entry = sample_entry();
    err_entry.request_id = "test-err".into();
    err_entry.status_code = 500;
    insert_request_log(&conn, &err_entry).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, Some("5xx"), None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status_code, 500);

    let results = query_request_log(&conn, 10, 0, None, None, None, Some("2xx"), None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status_code, 200);
}

#[test]
fn query_pagination() {
    let conn = in_memory_db();
    for i in 0..5 {
        let mut entry = sample_entry();
        entry.request_id = format!("test-{i}");
        insert_request_log(&conn, &entry).unwrap();
    }

    let page1 = query_request_log(&conn, 2, 0, None, None, None, None, None).unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = query_request_log(&conn, 2, 2, None, None, None, None, None).unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = query_request_log(&conn, 2, 4, None, None, None, None, None).unwrap();
    assert_eq!(page3.len(), 1);
}

#[test]
fn get_request_by_id_found() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let result = get_request_by_id(&conn, "test-123").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().request_id, "test-123");
}

#[test]
fn get_request_by_id_not_found() {
    let conn = in_memory_db();
    let result = get_request_by_id(&conn, "nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn purge_old_logs_removes_old_entries() {
    let conn = in_memory_db();

    let mut old = sample_entry();
    old.timestamp = "2020-01-01T00:00:00Z".into();
    insert_request_log(&conn, &old).unwrap();

    insert_request_log(&conn, &sample_entry()).unwrap();

    let purged = purge_old_logs(&conn, 1).unwrap();
    assert_eq!(purged, 1);

    let remaining = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].request_id, "test-123");
}

#[test]
fn insert_and_query_with_key_id_and_cost() {
    let conn = in_memory_db();
    let mut entry = sample_entry();
    entry.key_id = Some(42);
    entry.cost_usd = Some(0.0075);
    insert_request_log(&conn, &entry).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key_id, Some(42));
    assert!((results[0].cost_usd.unwrap() - 0.0075).abs() < 1e-12);

    let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(42)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].request_id, "test-123");

    let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(99)).unwrap();
    assert!(results.is_empty());

    let found = get_request_by_id(&conn, "test-123").unwrap().unwrap();
    assert_eq!(found.key_id, Some(42));
    assert!((found.cost_usd.unwrap() - 0.0075).abs() < 1e-12);
}

#[test]
fn insert_without_attribution_fields() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key_id, None);
    assert_eq!(results[0].cost_usd, None);
}

#[test]
fn count_requests_since_returns_zero_on_empty_log() {
    let conn = in_memory_db();
    let count = count_requests_since(&conn, 0).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn count_requests_since_counts_recent_entries() {
    let conn = in_memory_db();

    let recent = sample_entry();
    insert_request_log(&conn, &recent).unwrap();

    let mut old = sample_entry();
    old.request_id = "old-req".to_string();
    old.timestamp = "2020-01-01T00:00:00Z".to_string();
    insert_request_log(&conn, &old).unwrap();

    let since_2025: u64 = 1735689600; // 2025-01-01T00:00:00Z
    let count = count_requests_since(&conn, since_2025).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn observability_timeseries_groups_requests_into_buckets() {
    let conn = in_memory_db();

    let mut first = sample_entry();
    first.timestamp = "2099-01-01T00:00:05Z".into();
    first.input_tokens = Some(100);
    first.output_tokens = Some(25);
    first.cost_usd = Some(0.12);
    insert_request_log(&conn, &first).unwrap();

    let mut second = sample_entry();
    second.request_id = "test-456".into();
    second.timestamp = "2099-01-01T00:00:40Z".into();
    second.status_code = 503;
    second.input_tokens = Some(20);
    second.output_tokens = Some(4);
    second.cost_usd = Some(0.03);
    second.error_message = Some("upstream timeout".into());
    insert_request_log(&conn, &second).unwrap();

    let mut third = sample_entry();
    third.request_id = "test-789".into();
    third.timestamp = "2099-01-01T00:01:10Z".into();
    third.input_tokens = Some(7);
    third.output_tokens = Some(9);
    third.cost_usd = Some(0.01);
    insert_request_log(&conn, &third).unwrap();

    let buckets = query_request_timeseries(
        &conn,
        "2099-01-01T00:00:00Z",
        Some("2099-01-01T00:10:00Z"),
        None,
        None,
    )
    .unwrap();

    assert_eq!(buckets.len(), 2);
    assert_eq!(buckets[0].bucket_start, "2099-01-01T00:00:00Z");
    assert_eq!(buckets[0].requests_total, 2);
    assert_eq!(buckets[0].requests_error, 1);
    assert_eq!(buckets[0].input_tokens, 120);
    assert_eq!(buckets[0].output_tokens, 29);
    assert!((buckets[0].cost_usd - 0.15).abs() < 0.000001);
    assert_eq!(buckets[1].bucket_start, "2099-01-01T00:01:00Z");
    assert_eq!(buckets[1].requests_total, 1);
}

#[test]
fn observability_timeline_derives_request_start_time() {
    let conn = in_memory_db();

    let mut entry = sample_entry();
    entry.timestamp = "2099-01-01T00:00:10Z".into();
    entry.latency_ms = 1_500;
    insert_request_log(&conn, &entry).unwrap();

    let items = query_request_timeline(
        &conn,
        "2099-01-01T00:00:00Z",
        Some("2099-01-01T00:05:00Z"),
        None,
        None,
        10,
    )
    .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].finished_at, "2099-01-01T00:00:10.000Z");
    assert_eq!(items[0].started_at, "2099-01-01T00:00:08.500Z");
}

#[test]
fn observability_failure_breakdown_groups_similar_failures() {
    let conn = in_memory_db();

    let mut first = sample_entry();
    first.request_id = "test-fail-1".into();
    first.timestamp = "2099-01-01T00:00:10Z".into();
    first.status_code = 429;
    first.latency_ms = 500;
    first.error_message = Some("Upstream request req_abc123 throttled after 30s".into());
    first.error_kind = Some("rate_limit".into());
    insert_request_log(&conn, &first).unwrap();

    let mut second = sample_entry();
    second.request_id = "test-fail-2".into();
    second.timestamp = "2099-01-01T00:00:20Z".into();
    second.status_code = 429;
    second.latency_ms = 700;
    second.error_message = Some("Upstream request req_xyz789 throttled after 45s".into());
    second.error_kind = Some("rate_limit".into());
    insert_request_log(&conn, &second).unwrap();

    let mut third = sample_entry();
    third.request_id = "test-fail-3".into();
    third.timestamp = "2099-01-01T00:00:30Z".into();
    third.status_code = 500;
    third.error_message = Some("Backend crashed".into());
    third.error_kind = Some("upstream".into());
    insert_request_log(&conn, &third).unwrap();

    let mut fourth = sample_entry();
    fourth.request_id = "test-fail-4".into();
    fourth.timestamp = "2099-01-01T00:00:40Z".into();
    fourth.status_code = 429;
    fourth.error_message = Some("Upstream request req_qwe999 throttled after 60s".into());
    fourth.error_kind = Some("timeout".into());
    insert_request_log(&conn, &fourth).unwrap();

    let failures = query_failure_breakdown(
        &conn,
        "2099-01-01T00:00:00Z",
        Some("2099-01-01T01:00:00Z"),
        None,
        None,
        10,
    )
    .unwrap();

    assert_eq!(failures.len(), 3);
    assert_eq!(failures[0].error_kind.as_deref(), Some("rate_limit"));
    assert_eq!(failures[0].status_code, 429);
    assert_eq!(failures[0].count, 2);
    assert_eq!(failures[0].avg_latency_ms, 600);
    assert!(failures[0].summary.starts_with("Upstream request"));
}

#[test]
fn status_filter_parses_valid_inputs() {
    assert!(StatusFilter::parse("200").is_some());
    assert!(StatusFilter::parse("2xx").is_some());
    assert!(StatusFilter::parse("4xx").is_some());
    assert!(StatusFilter::parse("5xx").is_some());
    assert!(StatusFilter::parse("404").is_some());
}

#[test]
fn status_filter_rejects_invalid_inputs() {
    assert!(StatusFilter::parse("abc").is_none());
    assert!(StatusFilter::parse("2xx; DROP TABLE").is_none());
    assert!(StatusFilter::parse("").is_none());
    assert!(StatusFilter::parse("99999").is_none());
    assert!(StatusFilter::parse("-1").is_none());
}

#[test]
fn status_filter_exact_code_query() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let mut err_entry = sample_entry();
    err_entry.request_id = "test-404".into();
    err_entry.status_code = 404;
    insert_request_log(&conn, &err_entry).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, Some("404"), None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status_code, 404);
}

#[test]
fn status_filter_invalid_ignored() {
    let conn = in_memory_db();
    insert_request_log(&conn, &sample_entry()).unwrap();

    let results = query_request_log(&conn, 10, 0, None, None, None, Some("garbage"), None).unwrap();
    assert_eq!(results.len(), 1);
}
