/** Represents real-time usage and performance metrics for the proxy. */
export interface Metrics {
  /** The total number of requests received by the proxy. */
  total_requests: number
  /** The number of successful requests. */
  successful_requests: number
  /** The number of failed requests. */
  failed_requests: number
  /** The current request rate per minute. */
  requests_per_minute: number
  /** The p50 latency in milliseconds, if available. */
  p50_latency_ms: number | null
  /** The p95 latency in milliseconds, if available. */
  p95_latency_ms: number | null
  /** The percentage rate of request failures. */
  error_rate: number
  /** The number of streaming connection requests started. */
  streams_started: number
  /** The number of streaming connections completed. */
  streams_completed: number
  /** The number of streaming connections that failed. */
  streams_failed: number
  /** The number of streaming connections disconnected by the client. */
  streams_client_disconnected: number
  /** Requests where pxpipe text-to-image compression fired. */
  pxpipe_compressed_total: number
  /** Total PNG image blocks pxpipe emitted. */
  pxpipe_images_total: number
  /** Total source chars pxpipe replaced with images. */
  pxpipe_imaged_chars_total: number
  /** Requests where RTK tool-output compression fired. */
  rtk_compressed_total: number
  /** Total tool-result payloads RTK rewrote. */
  rtk_blocks_total: number
  /** Total source chars RTK removed from tool output. */
  rtk_saved_chars_total: number
}

/** A time-series data point for observability charts. */
export interface ObservabilityPoint {
  /** UNIX timestamp representing the bucket start time. */
  bucket_start: number
  /** Total requests in this bucket. */
  requests: number
  /** Total errors in this bucket. */
  errors: number
  /** Total input tokens processed in this bucket. */
  input_tokens: number
  /** Total output tokens generated in this bucket. */
  output_tokens: number
  /** Calculated cost in USD in this bucket. */
  cost_usd: number
}

/** Summary of error occurrences on a backend. */
export interface ObservabilityFailure {
  /** Class or classification of error. */
  error_kind: string
  /** Occurrence count. */
  count: number
  /** Last occurrence timestamp. */
  last_seen: string
  /** Last error message received. */
  last_message: string
}

/** Represents a single trace event in the observability timeline. */
export interface ObservabilityTimeline {
  /** Transaction request ID. */
  request_id: string
  /** ISO 8601 request timestamp. */
  timestamp: string
  /** Backend provider routed to. */
  backend: string
  /** Model mapped to. */
  model: string
  /** Latency in milliseconds. */
  latency_ms: number
  /** Request outcome status. */
  status: string
}

/** Observability stats summary response. */
export interface ObservabilityResponse {
  /** The time window in hours for metrics. */
  window_hours: number
  /** The name of the backend. */
  backend: string
  /** Total requests within the window. */
  total_requests: number
  /** Total errors within the window. */
  total_errors: number
  /** Total input tokens processed. */
  total_input_tokens: number
  /** Total output tokens generated. */
  total_output_tokens: number
  /** Total cost in USD. */
  total_cost_usd: number
  /** Historical metrics series points. */
  series: ObservabilityPoint[]
  /** Error breakdown summary. */
  failures: ObservabilityFailure[]
  /** Timeline events. */
  timeline: ObservabilityTimeline[]
}

/** Real-time metrics for a specific API route. */
export interface RouteMetrics {
  /** The request path/route. */
  path: string
  /** Number of requests per minute. */
  requests_per_min: number
  /** Percentage rate of failures. */
  error_rate: number
  /** Average latency in milliseconds. */
  avg_latency_ms: number
  /** The p95 latency in milliseconds. */
  p95_latency_ms: number
  /** Total number of requests. */
  total_requests: number
}

/** Time-series requests count point for a route. */
export interface TrafficSeriesPoint {
  /** UNIX timestamp representing the bucket start time. */
  bucket_start: number
  /** API path. */
  path: string
  /** Request count. */
  requests: number
}

/** Response containing traffic analytics. */
export interface TrafficResponse {
  /** Time window in hours. */
  window_hours: number
  /** Metrics per route. */
  routes: RouteMetrics[]
  /** Time-series data points. */
  series: TrafficSeriesPoint[]
}
