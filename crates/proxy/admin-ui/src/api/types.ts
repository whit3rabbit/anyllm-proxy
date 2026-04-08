// Mirrors the JSON shapes returned by /admin/api/* endpoints.
// Keep in sync with Rust structs in crates/proxy/src/admin/state.rs and routes/.

export interface ProxyStatus {
  configured: boolean
}

export interface Metrics {
  total_requests: number
  successful_requests: number
  failed_requests: number
  requests_per_minute: number
  p50_latency_ms: number | null
  p95_latency_ms: number | null
  error_rate: number
  streams_started: number
  streams_completed: number
  streams_failed: number
  streams_client_disconnected: number
}

export interface RequestLogEntry {
  request_id: string
  timestamp: string
  backend: string
  model_requested: string | null
  model_mapped: string | null
  status_code: number
  latency_ms: number
  input_tokens: number | null
  output_tokens: number | null
  is_streaming: boolean
  error_message: string | null
  error_kind: string | null
  key_id: number | null
  cost_usd: number | null
}

export interface RequestsResponse {
  requests: RequestLogEntry[]
  total: number
  page: number
  page_size: number
  has_more: boolean
}

export interface VirtualKey {
  id: number
  key_prefix: string
  description: string | null
  created_at: string
  expires_at: string | null
  revoked_at: string | null
  spend_limit: number | null
  rpm_limit: number | null
  tpm_limit: number | null
  total_spend: number
  total_requests: number
  total_tokens: number
  period_reset_at: string | null
  allowed_models: string[] | null
  status: 'active' | 'revoked' | 'expired' | 'override'
}

export interface KeySpend {
  id: number
  total_spend: number
  total_requests: number
  total_tokens: number
}

export interface Backend {
  name: string
  model: string
  provider: string
  status: string
  requests_total: number
  requests_ok: number
  requests_err: number
  p50_ms: number
  p95_ms: number
}

export interface ConfigEntry {
  key: string
  value: string
  updated_at: string
}

export interface ConfigResponse {
  entries: ConfigEntry[]
  env: Record<string, string>
}

export interface ObservabilityPoint {
  bucket_start: number
  requests: number
  errors: number
  input_tokens: number
  output_tokens: number
  cost_usd: number
}

export interface ObservabilityFailure {
  error_kind: string
  count: number
  last_seen: string
  last_message: string
}

export interface ObservabilityTimeline {
  request_id: string
  timestamp: string
  backend: string
  model: string
  latency_ms: number
  status: string
}

export interface ObservabilityResponse {
  window_hours: number
  backend: string
  total_requests: number
  total_errors: number
  total_input_tokens: number
  total_output_tokens: number
  total_cost_usd: number
  series: ObservabilityPoint[]
  failures: ObservabilityFailure[]
  timeline: ObservabilityTimeline[]
}

export interface ModelEntry {
  name: string
  provider: string
  model: string
  weight: number | null
  rpm: number | null
  tpm: number | null
}

export interface ModelsResponse {
  models: ModelEntry[]
  routing_strategy: string
}

export interface DiscoveredModel {
  id: string
  name: string | null
}

export interface DiscoverResponse {
  models: DiscoveredModel[]
  source: string
  auth_used: boolean
}

export interface AuditEntry {
  id: number
  timestamp: string
  action: string
  target_type: string
  target_id: string | null
  detail: string | null
  source_ip: string | null
}

export interface AuditResponse {
  entries: AuditEntry[]
  total: number
  has_more: boolean
}

// --- Traffic tab (new) ---

export interface RouteMetrics {
  path: string
  requests_per_min: number
  error_rate: number
  avg_latency_ms: number
  p95_latency_ms: number
  total_requests: number
}

export interface TrafficSeriesPoint {
  bucket_start: number
  path: string
  requests: number
}

export interface TrafficResponse {
  window_hours: number
  routes: RouteMetrics[]
  series: TrafficSeriesPoint[]
}

// --- Uptime tab (new) ---

export interface HistoryDay {
  date: string
  status: 'up' | 'down' | 'degraded' | 'no-data'
}

export interface ProxyUptimeInfo {
  started_at: number
  uptime_pct_30d: number
  history: HistoryDay[]
}

export interface BackendUptimeInfo {
  name: string
  status: 'up' | 'down' | 'unknown'
  last_checked_at: number | null
  last_latency_ms: number | null
  uptime_pct_30d: number
  history: HistoryDay[]
}

export interface UptimeResponse {
  proxy: ProxyUptimeInfo
  backends: BackendUptimeInfo[]
}

// --- Env file import / export ---

export interface EnvWarning {
  line: number | null
  key: string | null
  message: string
}

export interface EnvImportResponse {
  applied: number
  warnings: EnvWarning[]
}

export interface EnvImportError {
  hard_errors: string[]
  warnings: EnvWarning[]
}

// --- Provider catalog ---

export interface CatalogProvider {
  id: string
  name: string
  protocol: string
  auth: string
  default_base_url: string
  env_vars: string[]
}

// --- Managed backends ---

export interface ManagedBackend {
  id: string
  name: string
  provider_id: string
  api_key_set: boolean
  aws_creds_set: boolean
  api_base: string | null
  deployment: string | null
  api_version: string | null
  project: string | null
  region: string | null
  rpm: number | null
  tpm: number | null
  created_at: string
  updated_at: string
}

export interface CreateManagedBackendRequest {
  name: string
  provider_id: string
  api_key?: string
  api_base?: string
  deployment?: string
  api_version?: string
  project?: string
  region?: string
  aws_access_key_id?: string
  aws_secret_access_key?: string
  aws_session_token?: string
  rpm?: number
  tpm?: number
}

export type UpdateManagedBackendRequest = Partial<Omit<CreateManagedBackendRequest, 'name'>>

// --- WebSocket events ---

export type WSEvent =
  | { type: 'request_completed'; data: RequestLogEntry }
  | { type: 'metrics_snapshot'; data: Metrics }
  | { type: 'config_changed'; data: { key: string; value: string } }
  | { type: 'backend_health_changed'; data: { backend: string; status: 'up' | 'down'; latency_ms: number | null } }
