/** A single request transaction log entry. */
export interface RequestLogEntry {
  /** Unique transaction ID. */
  request_id: string
  /** ISO 8601 timestamp when the request was made. */
  timestamp: string
  /** The backend model provider targeted. */
  backend: string
  /** The model identifier requested by the client. */
  model_requested: string | null
  /** The actual model identifier routed to on the backend. */
  model_mapped: string | null
  /** HTTP response status code. */
  status_code: number
  /** Request duration in milliseconds. */
  latency_ms: number
  /** Number of input tokens processed. */
  input_tokens: number | null
  /** Number of output tokens generated. */
  output_tokens: number | null
  /** Whether the request was streamed. */
  is_streaming: boolean
  /** The raw error message returned by the backend, if any. */
  error_message: string | null
  /** Normalized error class/category. */
  error_kind: string | null
  /** The virtual key ID used for authorization, if any. */
  key_id: number | null
  /** Calculated transaction cost in USD. */
  cost_usd: number | null
}

/** Paginated list response for request log queries. */
export interface RequestsResponse {
  /** The page of request log entries. */
  requests: RequestLogEntry[]
  /** Maximum number of items returned. */
  limit: number
  /** Pagination offset. */
  offset: number
  /** True if more records are available. */
  has_more: boolean
}
