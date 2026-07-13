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
