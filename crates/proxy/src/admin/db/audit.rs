use super::common::chrono_now;
use rusqlite::{params, Connection};

/// A single audit log entry recording an admin mutation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub action: String,
    pub target_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
}

/// Insert an audit log entry with current UTC timestamp.
pub fn insert_audit_entry(conn: &Connection, entry: &AuditEntry) -> rusqlite::Result<()> {
    let ts = chrono_now();
    conn.execute(
        "INSERT INTO audit_log (timestamp, action, target_type, target_id, detail, source_ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ts,
            entry.action,
            entry.target_type,
            entry.target_id,
            entry.detail,
            entry.source_ip,
        ],
    )?;
    Ok(())
}

/// Query the audit log, returning entries in reverse chronological order.
pub fn query_audit_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    action: Option<&str>,
    target_type: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
) -> rusqlite::Result<Vec<AuditEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, action, target_type, target_id, detail, source_ip
         FROM audit_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(a) = action {
        sql.push_str(" AND action = ?");
        param_values.push(Box::new(a.to_string()));
    }
    if let Some(t) = target_type {
        sql.push_str(" AND target_type = ?");
        param_values.push(Box::new(t.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(u.to_string()));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(AuditEntry {
            id: Some(row.get(0)?),
            timestamp: Some(row.get(1)?),
            action: row.get(2)?,
            target_type: row.get(3)?,
            target_id: row.get(4)?,
            detail: row.get(5)?,
            source_ip: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::init_db(&conn).unwrap();
        conn
    }

    fn insert_audit(conn: &Connection, action: &str, target_type: &str, ts: &str) {
        conn.execute(
            "INSERT INTO audit_log (timestamp, action, source_ip, target_type, target_id, detail) \
             VALUES (?1, ?2, '127.0.0.1', ?3, NULL, NULL)",
            params![ts, action, target_type],
        )
        .unwrap();
    }

    #[test]
    fn audit_log_insert_and_query() {
        let conn = in_memory_db();
        let entry1 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_created".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: Some("description=test key, prefix=sk-vk-abc".into()),
            source_ip: Some("127.0.0.1".into()),
        };
        let entry2 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_revoked".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: None,
            source_ip: None,
        };
        insert_audit_entry(&conn, &entry1).unwrap();
        insert_audit_entry(&conn, &entry2).unwrap();

        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].action, "key_revoked");
        assert_eq!(results[1].action, "key_created");
        assert!(results[0].id.unwrap() > results[1].id.unwrap());
        assert!(results[0].timestamp.is_some());
        assert_eq!(results[1].target_id.as_deref(), Some("42"));
        assert_eq!(
            results[1].detail.as_deref(),
            Some("description=test key, prefix=sk-vk-abc")
        );
        assert_eq!(results[1].source_ip.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn audit_log_empty_returns_empty_vec() {
        let conn = in_memory_db();
        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn audit_log_pagination() {
        let conn = in_memory_db();
        for i in 0..5 {
            insert_audit_entry(
                &conn,
                &AuditEntry {
                    id: None,
                    timestamp: None,
                    action: format!("action_{i}"),
                    target_type: "test".into(),
                    target_id: None,
                    detail: None,
                    source_ip: None,
                },
            )
            .unwrap();
        }
        let page1 = query_audit_log(&conn, 2, 0, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);
        let page2 = query_audit_log(&conn, 2, 2, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);
        let page3 = query_audit_log(&conn, 2, 4, None, None, None, None).unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn audit_filter_by_action() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, Some("key_created"), None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_created");
    }

    #[test]
    fn audit_filter_by_target_type() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "config_changed", "config", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, None, Some("config"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target_type, "config");
    }

    #[test]
    fn audit_filter_since_until() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-03T00:00:00Z");
        insert_audit(&conn, "key_updated", "virtual_key", "2099-01-05T00:00:00Z");

        let results = query_audit_log(
            &conn,
            10,
            0,
            None,
            None,
            Some("2099-01-02T00:00:00Z"),
            Some("2099-01-04T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_revoked");
    }
}
