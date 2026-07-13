import type { RequestLogEntry } from './requests'
import type { Metrics } from './metrics'

/** Types of events sent over the admin WebSocket channel. */
export type WSEvent =
  | { type: 'request_completed'; data: RequestLogEntry }
  | { type: 'metrics_snapshot'; data: Metrics }
  | { type: 'config_changed'; data: { key: string; value: string } }
  | { type: 'backend_health_changed'; data: { backend: string; status: 'up' | 'down'; latency_ms: number | null } }
