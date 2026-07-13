// Mirrors the JSON shapes returned by /admin/api/* endpoints.
// Keep in sync with Rust structs in crates/proxy/src/admin/state.rs and routes/.

/** Represents the status and configuration details of the proxy server. */
export interface ProxyStatus {
  /** True if the proxy backend has been configured. */
  configured: boolean
  /** The port number the proxy listens on for client requests. */
  proxy_port: number
  /** Whether the proxy is currently running and accepting connections. */
  proxy_running: boolean
}

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

/** Represents a virtual API key configuration and its associated limits. */
export interface VirtualKey {
  /** Unique database ID. */
  id: number
  /** The prefix of the key shown to users. */
  key_prefix: string
  /** Optional descriptive note. */
  description: string | null
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 expiration timestamp, if set. */
  expires_at: string | null
  /** ISO 8601 revocation timestamp, if revoked. */
  revoked_at: string | null
  /** Spend limit in USD. */
  spend_limit: number | null
  /** Monthly budget limit in USD. */
  max_budget_usd: number | null
  /** Duration of the budget period (e.g. 'monthly'). */
  budget_duration: string | null
  /** Requests-per-minute limit. */
  rpm_limit: number | null
  /** Tokens-per-minute limit. */
  tpm_limit: number | null
  /** Total spend in USD across the key lifetime. */
  total_spend: number
  /** Total count of requests made with this key. */
  total_requests: number
  /** Total count of tokens processed. */
  total_tokens: number
  /** ISO 8601 reset timestamp for the current budget period. */
  period_reset_at: string | null
  /** List of model names this key is restricted to, if any. */
  allowed_models: string[] | null
  /** List of route names this key is restricted to, if any. */
  allowed_routes: string[] | null
  /** Active status of the key. */
  status: 'active' | 'revoked' | 'expired' | 'override'
  /** Spend in USD during the current budget period. */
  period_spend_usd: number
}

/** Spent token and requests details for a virtual key. */
export interface KeySpend {
  /** Unique key database ID. */
  id: number
  /** Total spend in USD. */
  total_spend: number
  /** Total request count. */
  total_requests: number
  /** Total token count. */
  total_tokens: number
}

/** Represents a backend endpoint model and its metrics. */
export interface Backend {
  /** Name of the backend. */
  name: string
  /** Model mapped for heavy workloads. */
  big_model: string
  /** Model mapped for light workloads. */
  small_model: string
  /** Request outcome counters. */
  metrics: {
    requests_total: number
    requests_success: number
    requests_error: number
  }
}

/** Represents a single configuration entry key-value pair. */
export interface ConfigEntry {
  /** Configuration key. */
  key: string
  /** Configuration value. */
  value: string
  /** Timestamp when updated. */
  updated_at: string
}

/** Represents the full system configuration and environment variables. */
export interface ConfigResponse {
  /** List of config database overrides. */
  entries: ConfigEntry[]
  /** Server environment variables map. */
  env: Record<string, string>
  /** System logging level. */
  log_level: string
  /** Whether requests and responses are logged. */
  log_bodies: boolean
  /** Whether credentials are redacted from logs. */
  redact_secrets: boolean
  /** Whether to attempt to repair broken thinking blocks in Anthropic streams. */
  anthropic_thinking_repair: boolean
  /** True if pxpipe image compression is enabled. */
  pxpipe_compress: boolean
  /** CSV of model bases in pxpipe compression scope. */
  pxpipe_models: string
  /** Vision-capable Claude models offered as per-model scope toggles. */
  pxpipe_available_models: string[]
  /** Whether RTK tool-output compression is active. */
  rtk_compress: boolean
  /** CSV of model bases in RTK compression scope (empty = all models). */
  rtk_models: string
  /** Whether to pass client authorization headers to backend. */
  forward_client_auth: boolean
  /** Tool guardrail mode configured. */
  tool_guardrail_mode: string
  /** Prompt-compression optimizer mode: 'off' | 'shadow' | 'live'. */
  optimizer_mode: string
  /** Mappings of backend names to their configured big/small models. */
  backends: Record<string, { big_model: string; small_model: string }>
  /** List of keys whose overrides are active. */
  overridden_keys: string[]
}

/** Status of the optional LLMLingua-2 ONNX model artifact (optimizer scorer tier). */
export interface OptimizerModelStatus {
  /** Proxy built with the `optimizer-onnx` feature. When false the tier is inert. */
  compiled_in: boolean
  /** Verified model artifact is present on disk. */
  present: boolean
  /** A download+verify is currently in flight. */
  downloading: boolean
  /** Last download error, if any. */
  error: string | null
  /** Pinned sha256 the artifact is verified against. */
  sha256: string
  /** Expected download size in bytes. */
  size_bytes: number
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

/** Represents a single model configuration entry. */
export interface ModelEntry {
  /** The unique name/identifier of the model. */
  model_name: string
  /** The number of active deployments for this model. */
  deployments: number
}

/** Response shape for models list requests. */
export interface ModelsResponse {
  /** List of model configurations. */
  models: ModelEntry[]
  /** Router strategy (e.g. priority, failover). */
  strategy: string | null
  /** Optional descriptive note. */
  note?: string
}

/** Represents a model discovered from a backend API. */
export interface DiscoveredModel {
  /** The model identifier. */
  id: string
  /** Optional human-readable name. */
  name: string | null
}

/** Response containing discovered models. */
export interface DiscoverResponse {
  /** List of discovered models. */
  models: DiscoveredModel[]
  /** Source or method used for discovery. */
  source: string
  /** True if authorization was used. */
  auth_used: boolean
}

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

// --- Traffic tab (new) ---

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

// --- Uptime tab (new) ---

/** A single day's uptime status. */
export interface HistoryDay {
  /** Date in YYYY-MM-DD format. */
  date: string
  /** Availability status. */
  status: 'up' | 'down' | 'degraded' | 'no-data'
}

/** Uptime info for the proxy itself. */
export interface ProxyUptimeInfo {
  /** UNIX timestamp when the proxy started. */
  started_at: number
  /** Uptime percentage over the last 30 days. */
  uptime_pct_30d: number
  /** Daily uptime history. */
  history: HistoryDay[]
}

/** Uptime info for a backend endpoint. */
export interface BackendUptimeInfo {
  /** Name of the backend. */
  name: string
  /** Current connection status. */
  status: 'up' | 'down' | 'unknown'
  /** UNIX timestamp of the last health check. */
  last_checked_at: number | null
  /** Last checked latency in milliseconds. */
  last_latency_ms: number | null
  /** 30-day uptime percentage. */
  uptime_pct_30d: number
  /** Daily uptime history. */
  history: HistoryDay[]
}

/** Full uptime summary response. */
export interface UptimeResponse {
  /** Proxy server uptime details. */
  proxy: ProxyUptimeInfo
  /** Backend endpoints uptime details. */
  backends: BackendUptimeInfo[]
}

// --- Env file import / export ---

/** Warning message generated during env import. */
export interface EnvWarning {
  /** Affected line number. */
  line: number | null
  /** Affected environment variable key. */
  key: string | null
  /** Warning message. */
  message: string
}

/** Response detailing the result of importing an env file. */
export interface EnvImportResponse {
  /** Count of imported variables applied. */
  applied: number
  /** Non-fatal warnings generated. */
  warnings: EnvWarning[]
}

/** Error payload for env file imports. */
export interface EnvImportError {
  /** Hard/blocking validation errors. */
  hard_errors: string[]
  /** Non-fatal warnings. */
  warnings: EnvWarning[]
}

// --- Provider catalog ---

/** Details of a provider available in the LiteLLM catalog. */
export interface CatalogProvider {
  /** Unique provider identifier. */
  id: string
  /** User-facing display name. */
  display_name: string
  /** Keep for backwards compatibility. */
  name?: string
  /** Communication protocol. */
  protocol: string
  /** Auth mechanism. */
  auth: string
  /** Proxy integration status. */
  status: 'implemented' | 'wired' | 'stub'
  /** Default base URL. */
  default_base_url: string
  /** Expected environment variables for API keys. */
  env_vars: string[]
  /** LiteLLM prefix string. */
  litellm_prefix: string
  /** Supported capabilities. */
  capabilities: {
    chat_completions: boolean
    streaming: boolean
    tool_use: boolean
    embeddings: boolean
    vision: boolean
    batch: boolean
  }
  /** Total count of models. */
  model_count: number
  /** Count of models currently cached. */
  cached_model_count: number
  /** UNIX timestamp of the last cache refresh. */
  last_refreshed: number | null
}

// --- Managed backends ---

/** Represents a backend credentials deployment managed by the admin. */
export interface ManagedBackend {
  /** Unique database ID. */
  id: string
  /** Unique backend name. */
  name: string
  /** Provider ID. */
  provider_id: string
  /** True if the API key is configured. */
  api_key_set: boolean
  /** True if AWS credentials are set (for AWS Bedrock, etc.). */
  aws_creds_set: boolean
  /** Base URL of the API endpoint. */
  api_base: string | null
  /** Optional deployment name (e.g. Azure deployment name). */
  deployment: string | null
  /** Optional API version. */
  api_version: string | null
  /** Optional cloud project ID. */
  project: string | null
  /** Optional cloud region (e.g. AWS/Azure region). */
  region: string | null
  /** Rate limit: requests per minute override. */
  rpm: number | null
  /** Rate limit: tokens per minute override. */
  tpm: number | null
  /** True if this backend is enabled. */
  enabled: boolean
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 last update timestamp. */
  updated_at: string
}

/** Response containing managed backends. */
export interface ManagedBackendsResponse {
  /** List of managed backends. */
  backends: ManagedBackend[]
}

/** Request payload for creating a new managed backend. */
export interface CreateManagedBackendRequest {
  /** Unique name for the backend. */
  name: string
  /** Catalog provider ID. */
  provider_id: string
  /** API key credential string. */
  api_key?: string
  /** API base URL. */
  api_base?: string
  /** API deployment name. */
  deployment?: string
  /** API version. */
  api_version?: string
  /** Cloud project ID. */
  project?: string
  /** Cloud region. */
  region?: string
  /** AWS access key ID. */
  aws_access_key_id?: string
  /** AWS secret access key. */
  aws_secret_access_key?: string
  /** AWS session token. */
  aws_session_token?: string
  /** Requests per minute limit. */
  rpm?: number
  /** Tokens per minute limit. */
  tpm?: number
  /** Active status toggle. */
  enabled?: boolean
}

/** Request payload for updating an existing managed backend. */
export type UpdateManagedBackendRequest = Partial<Omit<CreateManagedBackendRequest, 'name'>>

// --- Routes ---

/** Represents a configured API routing definition. */
export interface Route {
  /** Unique route database ID. */
  id: string
  /** Route name. */
  name: string
  /** Optional description. */
  description: string | null
  /** Load balancing strategy. */
  strategy: string
  /** Requests per minute limit. */
  rpm: number | null
  /** Tokens per minute limit. */
  tpm: number | null
  /** Cost budget in USD. */
  budget_usd: number | null
  /** Active status toggle. */
  enabled: boolean
  /** Tool guardrail mode override. */
  guardrail_mode: string | null
  /** pxpipe image compression toggle override. */
  pxpipe_compress: boolean | null
  /** pxpipe models CSV override. */
  pxpipe_models: string | null
  /** Secret redaction toggle override. */
  redact_secrets: boolean | null
  /** Position order in the router. */
  position: number
  /** Count of providers assigned to this route. */
  provider_count: number
  /** ISO 8601 creation timestamp. */
  created_at: string
  /** ISO 8601 last update timestamp. */
  updated_at: string
}

/** Response containing routing configs. */
export interface RoutesResponse {
  /** List of routes. */
  routes: Route[]
}

/** Request payload to create a new route. */
export interface CreateRouteRequest {
  /** Unique route name. */
  name: string
  /** Route description. */
  description?: string
  /** Load balancing strategy. */
  strategy?: string
  /** Requests per minute limit. */
  rpm?: number
  /** Tokens per minute limit. */
  tpm?: number
  /** Cost budget in USD. */
  budget_usd?: number
  /** Active status toggle. */
  enabled?: boolean
  /** Tool guardrail mode override. */
  guardrail_mode?: string | null
  /** pxpipe image compression toggle override. */
  pxpipe_compress?: boolean | null
  /** pxpipe models CSV override. */
  pxpipe_models?: string | null
  /** Secret redaction toggle override. */
  redact_secrets?: boolean | null
  /** Position order. */
  position?: number
}

/** Request payload to update an existing route. */
export type UpdateRouteRequest = Partial<CreateRouteRequest>

/** Represents a provider mapped to a route. */
export interface RouteProvider {
  /** Unique assignment ID. */
  id: string
  /** Target route ID. */
  route_id: string
  /** Backend ID. */
  backend_id: string
  /** Name of the backend. */
  backend_name: string
  /** Catalog provider ID. */
  provider_id: string
  /** Supported models list. */
  models: string[]
  /** Evaluation priority. */
  priority: number
  /** Active status. */
  enabled: boolean
}

/** Response containing route provider mappings. */
export interface RouteProvidersResponse {
  /** List of mappings. */
  providers: RouteProvider[]
}

/** Request payload to add a provider assignment to a route. */
export interface AddRouteProviderRequest {
  /** Target backend ID. */
  backend_id: string
  /** Supported models list. */
  models?: string[]
  /** Priority position. */
  priority?: number
  /** Active status. */
  enabled?: boolean
}

/** Request payload to update a provider assignment configuration. */
export interface UpdateRouteProviderRequest {
  /** Supported models list. */
  models?: string[]
  /** Priority position. */
  priority?: number
  /** Active status. */
  enabled?: boolean
}

/** Request payload to reorder provider mappings. */
export interface ReorderRouteProvidersRequest {
  /** Ordered list of provider assignment IDs. */
  provider_ids: string[]
}

// --- WebSocket events ---

/** Types of events sent over the admin WebSocket channel. */
export type WSEvent =
  | { type: 'request_completed'; data: RequestLogEntry }
  | { type: 'metrics_snapshot'; data: Metrics }
  | { type: 'config_changed'; data: { key: string; value: string } }
  | { type: 'backend_health_changed'; data: { backend: string; status: 'up' | 'down'; latency_ms: number | null } }

/** Details of a model available in the LiteLLM catalog. */
export interface CatalogModel {
  /** Unique model identifier. */
  id: string
  /** Maximum context window size in tokens. */
  context_window: number
  /** Maximum output tokens. */
  max_output_tokens: number
  /** Availability status of the model. */
  status: 'available' | 'deprecated' | 'stub'
  /** Capabilities flags. */
  capabilities: {
    streaming: boolean
    tool_use: boolean
    vision: boolean
    extended_thinking: boolean
  }
  /** Pricing per million tokens. */
  pricing: {
    input_per_million_tokens: number
    output_per_million_tokens: number
  } | null
}

/** Response containing model lists. */
export interface CatalogModelsResponse {
  /** Catalog provider ID. */
  provider_id: string
  /** True if this provider has models. */
  has_models: boolean
  /** List of models. */
  models: CatalogModel[]
}
