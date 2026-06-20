use super::*;

pub(super) fn batch_job_from_row(row: &rusqlite::Row) -> BatchJob {
    let status_str: String = row.get(1).unwrap_or_default();
    let exec_mode_str: String = row.get(2).unwrap_or_default();
    let provider: Option<String> = row.get(3).unwrap_or(None);
    let metadata_str: Option<String> = row.get(8).unwrap_or(None);

    let execution_mode = match exec_mode_str.as_str() {
        "native" => ExecutionMode::Native {
            provider: provider.unwrap_or_else(|| "unknown".into()),
        },
        _ => ExecutionMode::ProxyNative,
    };

    BatchJob {
        id: BatchId(row.get(0).unwrap_or_default()),
        status: BatchStatus::from_str_status(&status_str),
        execution_mode,
        priority: row.get::<_, i64>(4).unwrap_or(0) as u8,
        key_id: row.get(5).unwrap_or(None),
        input_file_id: row.get(6).unwrap_or_default(),
        webhook_url: row.get(7).unwrap_or(None),
        metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        request_counts: RequestCounts {
            total: row.get::<_, i64>(9).unwrap_or(0) as u32,
            processing: row.get::<_, i64>(10).unwrap_or(0) as u32,
            succeeded: row.get::<_, i64>(11).unwrap_or(0) as u32,
            failed: row.get::<_, i64>(12).unwrap_or(0) as u32,
            cancelled: row.get::<_, i64>(13).unwrap_or(0) as u32,
            expired: row.get::<_, i64>(14).unwrap_or(0) as u32,
        },
        created_at: row.get(15).unwrap_or_default(),
        started_at: row.get(16).unwrap_or(None),
        completed_at: row.get(17).unwrap_or(None),
        expires_at: row.get(18).unwrap_or_default(),
    }
}

pub(super) fn batch_item_from_row(row: &rusqlite::Row) -> BatchItem {
    let status_str: String = row.get(3).unwrap_or_default();
    let model: String = row.get(4).unwrap_or_default();
    let body_str: String = row.get(5).unwrap_or_default();
    let source_fmt_str: String = row.get(6).unwrap_or_default();
    let result_status: Option<i64> = row.get(7).unwrap_or(None);
    let result_body_str: Option<String> = row.get(8).unwrap_or(None);

    let source_format =
        serde_json::from_str::<SourceFormat>(&source_fmt_str).unwrap_or(SourceFormat::OpenAI);

    let body = serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

    let result = match (result_status, result_body_str) {
        (Some(code), Some(body_s)) => {
            let body_val = serde_json::from_str(&body_s).unwrap_or(serde_json::Value::Null);
            Some(BatchItemResult {
                status_code: code as u16,
                body: body_val,
            })
        }
        _ => None,
    };

    BatchItem {
        id: ItemId(row.get(0).unwrap_or_default()),
        batch_id: BatchId(row.get(1).unwrap_or_default()),
        custom_id: row.get(2).unwrap_or_default(),
        status: ItemStatus::from_str_status(&status_str),
        request: BatchItemRequest {
            model,
            body,
            source_format,
        },
        result,
        attempts: row.get::<_, i64>(9).unwrap_or(0) as u8,
        max_retries: row.get::<_, i64>(10).unwrap_or(3) as u8,
        last_error: row.get(11).unwrap_or(None),
        next_retry_at: row.get(12).unwrap_or(None),
        lease_id: row.get(13).unwrap_or(None),
        lease_expires_at: row.get(14).unwrap_or(None),
        idempotency_key: row.get(15).unwrap_or(None),
        created_at: row.get(16).unwrap_or_default(),
        completed_at: row.get(17).unwrap_or(None),
    }
}

pub(super) fn row_to_job(
    conn: &Connection,
    batch_id: &str,
) -> Result<Option<BatchJob>, QueueError> {
    let mut stmt = conn.prepare(
        "SELECT batch_id, status, execution_mode, provider, priority,
            key_id, input_file_id, webhook_url, metadata,
            total, processing, succeeded, failed, cancelled, expired,
            created_at, started_at, completed_at, expires_at
         FROM batch_job WHERE batch_id = ?1",
    )?;
    let mut rows = stmt.query(params![batch_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(batch_job_from_row(row)))
    } else {
        Ok(None)
    }
}
