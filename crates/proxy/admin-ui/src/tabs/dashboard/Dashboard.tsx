import { useMetrics } from '../../api/queries'
import LiveFeed from '../../components/feed/LiveFeed'
import { AdminSurface, AnimatedMetric } from '../../components/shared/Performative'
import InfoTip from '../../components/shared/InfoTip'
import ObservabilityPanel from './ObservabilityPanel'

export default function Dashboard() {
  const { data: m } = useMetrics()

  return (
    <div>
      <div className="stats-row">
        <AdminSurface className="stat">
          <div className="stat-label">Requests/min</div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.requests_per_minute} precision={1} format={(n) => n.toFixed(1)} /> : '—'}
          </div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">Error Rate</div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.error_rate * 100} precision={1} format={(n) => `${n.toFixed(1)}%`} /> : '—'}
          </div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">P50 Latency<InfoTip text="Median response latency — half of requests were faster than this." /></div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.p50_latency_ms ?? 0} format={(n) => `${Math.round(n)}ms`} /> : '—'}
          </div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">P95 Latency<InfoTip text="95th-percentile latency — 95% of requests were faster than this. Captures tail slowness." /></div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.p95_latency_ms ?? 0} format={(n) => `${Math.round(n)}ms`} /> : '—'}
          </div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">Total Requests</div>
          <div className="stat-value">
            <AnimatedMetric value={m?.total_requests ?? 0} />
          </div>
        </AdminSurface>
      </div>
      <div className="stats-row" style={{ marginBottom: 16 }}>
        <AdminSurface className="stat">
          <div className="stat-label">Streams Started</div>
          <div className="stat-value"><AnimatedMetric value={m?.streams_started ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">Completed</div>
          <div className="stat-value ok"><AnimatedMetric value={m?.streams_completed ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">Failed</div>
          <div className="stat-value" style={{ color: 'var(--err)' }}><AnimatedMetric value={m?.streams_failed ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">Client Disconnects</div>
          <div className="stat-value" style={{ color: 'var(--warn)' }}><AnimatedMetric value={m?.streams_client_disconnected ?? 0} /></div>
        </AdminSurface>
      </div>
      {(m?.pxpipe_compressed_total ?? 0) > 0 && (
        <div className="stats-row" style={{ marginBottom: 16 }}>
          <AdminSurface className="stat">
            <div className="stat-label">Image-Compressed Requests</div>
            <div className="stat-value"><AnimatedMetric value={m?.pxpipe_compressed_total ?? 0} /></div>
          </AdminSurface>
          <AdminSurface className="stat">
            <div className="stat-label">Images Emitted</div>
            <div className="stat-value"><AnimatedMetric value={m?.pxpipe_images_total ?? 0} /></div>
          </AdminSurface>
          <AdminSurface className="stat">
            <div className="stat-label">Chars Imaged</div>
            <div className="stat-value"><AnimatedMetric value={m?.pxpipe_imaged_chars_total ?? 0} /></div>
          </AdminSurface>
        </div>
      )}
      <ObservabilityPanel />
      <div style={{ marginTop: 16 }}>
        <LiveFeed />
      </div>
    </div>
  )
}
