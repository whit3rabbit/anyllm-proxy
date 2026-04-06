import { useMetrics } from '../../api/queries'
import LiveFeed from '../../components/feed/LiveFeed'
import ObservabilityPanel from './ObservabilityPanel'

export default function Dashboard() {
  const { data: m } = useMetrics()

  return (
    <div>
      <div className="stats-row">
        <div className="stat">
          <div className="stat-label">Requests/min</div>
          <div className="stat-value">{m ? m.requests_per_minute.toFixed(1) : '—'}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Error Rate</div>
          <div className="stat-value">{m ? `${(m.error_rate * 100).toFixed(1)}%` : '—'}</div>
        </div>
        <div className="stat">
          <div className="stat-label">P50 Latency</div>
          <div className="stat-value">{m ? `${m.p50_latency_ms ?? 0}ms` : '—'}</div>
        </div>
        <div className="stat">
          <div className="stat-label">P95 Latency</div>
          <div className="stat-value">{m ? `${m.p95_latency_ms ?? 0}ms` : '—'}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Total Requests</div>
          <div className="stat-value">{m ? m.total_requests.toLocaleString() : '0'}</div>
        </div>
      </div>
      <div className="stats-row" style={{ marginBottom: 16 }}>
        <div className="stat">
          <div className="stat-label">Streams Started</div>
          <div className="stat-value">{m?.streams_started ?? 0}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Completed</div>
          <div className="stat-value ok">{m?.streams_completed ?? 0}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Failed</div>
          <div className="stat-value" style={{ color: 'var(--err)' }}>{m?.streams_failed ?? 0}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Client Disconnects</div>
          <div className="stat-value" style={{ color: 'var(--warn)' }}>{m?.streams_client_disconnected ?? 0}</div>
        </div>
      </div>
      <ObservabilityPanel />
      <div style={{ marginTop: 16 }}>
        <LiveFeed />
      </div>
    </div>
  )
}
