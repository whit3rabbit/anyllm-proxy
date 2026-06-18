use super::*;
use crate::db::init_batch_engine_tables;

async fn test_queue() -> SqliteQueue {
    let conn = Connection::open_in_memory().unwrap();
    init_batch_engine_tables(&conn).unwrap();
    SqliteQueue::new(Arc::new(Mutex::new(conn)))
}

fn make_job(id: &str) -> (BatchJob, Vec<BatchItem>) {
    let batch_id = BatchId(id.into());
    let now = crate::db::now_iso8601();
    let job = BatchJob {
        id: batch_id.clone(),
        status: BatchStatus::Queued,
        execution_mode: ExecutionMode::ProxyNative,
        priority: 0,
        key_id: None,
        input_file_id: "file-test".into(),
        webhook_url: None,
        metadata: None,
        request_counts: RequestCounts {
            total: 2,
            ..Default::default()
        },
        created_at: now.clone(),
        started_at: None,
        completed_at: None,
        expires_at: now.clone(),
    };
    let items = vec![
        BatchItem {
            id: ItemId(format!("{id}_item_1")),
            batch_id: batch_id.clone(),
            custom_id: "req-1".into(),
            status: ItemStatus::Pending,
            request: BatchItemRequest {
                model: "gpt-4o".into(),
                body: serde_json::json!({"messages": []}),
                source_format: SourceFormat::OpenAI,
            },
            result: None,
            attempts: 0,
            max_retries: 3,
            last_error: None,
            next_retry_at: None,
            lease_id: None,
            lease_expires_at: None,
            idempotency_key: None,
            created_at: now.clone(),
            completed_at: None,
        },
        BatchItem {
            id: ItemId(format!("{id}_item_2")),
            batch_id: batch_id.clone(),
            custom_id: "req-2".into(),
            status: ItemStatus::Pending,
            request: BatchItemRequest {
                model: "gpt-4o".into(),
                body: serde_json::json!({"messages": []}),
                source_format: SourceFormat::OpenAI,
            },
            result: None,
            attempts: 0,
            max_retries: 3,
            last_error: None,
            next_retry_at: None,
            lease_id: None,
            lease_expires_at: None,
            idempotency_key: None,
            created_at: now.clone(),
            completed_at: None,
        },
    ];
    (job, items)
}

#[tokio::test]
async fn enqueue_and_get() {
    let q = test_queue().await;
    let (job, items) = make_job("batch_test1");
    q.enqueue(&job, &items).await.unwrap();

    let fetched = q.get(&BatchId("batch_test1".into())).await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.status, BatchStatus::Queued);
    assert_eq!(fetched.request_counts.total, 2);
}

#[tokio::test]
async fn get_nonexistent() {
    let q = test_queue().await;
    let fetched = q.get(&BatchId("nope".into())).await.unwrap();
    assert!(fetched.is_none());
}

#[tokio::test]
async fn cancel_queued_job() {
    let q = test_queue().await;
    let (job, items) = make_job("batch_cancel");
    q.enqueue(&job, &items).await.unwrap();

    let cancelled = q.cancel(&BatchId("batch_cancel".into())).await.unwrap();
    assert_eq!(cancelled.status, BatchStatus::Cancelled);
}

#[tokio::test]
async fn list_with_pagination() {
    let q = test_queue().await;
    for i in 0..5 {
        let (job, items) = make_job(&format!("batch_list_{i}"));
        q.enqueue(&job, &items).await.unwrap();
    }

    let all = q.list(None, None, 10).await.unwrap();
    assert_eq!(all.len(), 5);

    let page = q.list(None, None, 2).await.unwrap();
    assert_eq!(page.len(), 2);
}

#[tokio::test]
async fn list_filters_by_key_id() {
    let q = test_queue().await;
    let (mut job1, items1) = make_job("batch_key_1");
    job1.key_id = Some(1);
    q.enqueue(&job1, &items1).await.unwrap();

    let (mut job2, items2) = make_job("batch_key_2");
    job2.key_id = Some(2);
    q.enqueue(&job2, &items2).await.unwrap();

    let scoped = q.list(Some(1), None, 10).await.unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].id.0, "batch_key_1");

    let unscoped = q.list(None, None, 10).await.unwrap();
    assert_eq!(unscoped.len(), 2);
}

#[tokio::test]
async fn get_items() {
    let q = test_queue().await;
    let (job, items) = make_job("batch_items");
    q.enqueue(&job, &items).await.unwrap();

    let fetched = q.get_items(&BatchId("batch_items".into())).await.unwrap();
    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].custom_id, "req-1");
    assert_eq!(fetched[1].custom_id, "req-2");
}

#[tokio::test]
async fn complete_item_and_batch() {
    let q = test_queue().await;
    let (job, items) = make_job("batch_complete");
    q.enqueue(&job, &items).await.unwrap();

    // Complete both items.
    let result = BatchItemResult {
        status_code: 200,
        body: serde_json::json!({"id": "resp-1"}),
    };
    q.complete_item(&ItemId("batch_complete_item_1".into()), result.clone())
        .await
        .unwrap();
    q.complete_item(&ItemId("batch_complete_item_2".into()), result)
        .await
        .unwrap();

    assert!(q
        .is_batch_complete(&BatchId("batch_complete".into()))
        .await
        .unwrap());

    q.complete_batch(&BatchId("batch_complete".into()))
        .await
        .unwrap();
    let job = q
        .get(&BatchId("batch_complete".into()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.status, BatchStatus::Completed);
    assert_eq!(job.request_counts.succeeded, 2);
}
