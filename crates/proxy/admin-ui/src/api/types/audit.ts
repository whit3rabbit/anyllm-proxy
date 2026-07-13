/** A single audit log entry tracking administrative actions. */
export interface AuditEntry {
  /** Unique database ID. */
  id: number
  /** ISO 8601 action timestamp. */
  timestamp: string
  /** The action performed. */
  action: string
  /** Type of target resource affected. */
  target_type: string
  /** ID of target resource affected. */
  target_id: string | null
  /** Detailed changes/payload. */
  detail: string | null
  /** Source IP of the requester. */
  source_ip: string | null
}

/** Paginated list response for audit log queries. */
export interface AuditResponse {
  /** List of audit log entries. */
  entries: AuditEntry[]
  /** Maximum number of items returned. */
  limit: number
  /** Pagination offset. */
  offset: number
  /** True if more records are available. */
  has_more: boolean
}
