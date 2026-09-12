/** Represents the status and configuration details of the proxy server. */
export interface ProxyStatus {
  /** True if the proxy backend has been configured. */
  configured: boolean
  /** The port number the proxy listens on for client requests. */
  proxy_port: number
  /** Whether the proxy is currently running and accepting connections. */
  proxy_running: boolean
  /**
   * Effective proxy auth posture. "keys": a key is required. "open_relay": any
   * key accepted on all interfaces. "auth_required": no authentication source is
   * configured, so all requests are rejected. Drives the top-of-app warning banner.
   */
  auth_mode: 'keys' | 'open_relay' | 'auth_required'
  /** Number of distinct static PROXY_API_KEYS entries. */
  proxy_key_count: number
}
