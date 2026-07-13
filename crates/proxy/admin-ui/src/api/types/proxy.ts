/** Represents the status and configuration details of the proxy server. */
export interface ProxyStatus {
  /** True if the proxy backend has been configured. */
  configured: boolean
  /** The port number the proxy listens on for client requests. */
  proxy_port: number
  /** Whether the proxy is currently running and accepting connections. */
  proxy_running: boolean
}
