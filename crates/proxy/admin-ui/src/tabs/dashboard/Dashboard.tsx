import { useMetrics } from '../../api/queries'
import LiveFeed from '../../components/feed/LiveFeed'
import { AdminSurface, AnimatedMetric } from '../../components/shared/Performative'
import InfoTip from '../../components/shared/InfoTip'
import Sparkline from '../../components/shared/Sparkline'
import { useMetricHistory, trendDelta } from '../../utils/useMetricHistory'
import ObservabilityPanel from './ObservabilityPanel'

/** Formats a trend delta as a signed, arrowed percentage. */
function DeltaTag({ series, goodWhenUp }: { series?: number[]; goodWhenUp: boolean }) {
  const delta = trendDelta(series)
  if (delta === null || Math.abs(delta) < 0.05) {
    return <span className="stat-delta" style={{ color: 'var(--text-3)' }}>—</span>
  }
  const up = delta > 0
  const good = up === goodWhenUp
  return (
    <span className="stat-delta" style={{ color: good ? 'var(--ok)' : 'var(--err)' }}>
      {up ? '▲' : '▼'} {Math.abs(delta).toFixed(1)}%
    </span>
  )
}

export default function Dashboard() {
  const { data: m } = useMetrics()

  // Rolling client-side history that feeds the stat-card sparklines + deltas.
  const hist = useMetricHistory(
    m,
    (s) => ({
      rpm: s.requests_per_minute,
      err: s.error_rate * 100,
      p50: s.p50_latency_ms ?? 0,
      p95: s.p95_latency_ms ?? 0,
    }),
    24,
  )

  return (
    <div>
      <div className="stats-row">
        <AdminSurface className="stat">
          <div className="stat-label">
            <span>Requests/min</span>
            <DeltaTag series={hist.rpm} goodWhenUp />
          </div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.requests_per_minute} precision={1} format={(n) => n.toFixed(1)} /> : '—'}
          </div>
          <Sparkline data={hist.rpm ?? []} color="var(--accent)" />
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">
            <span>Error Rate</span>
            <DeltaTag series={hist.err} goodWhenUp={false} />
          </div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.error_rate * 100} precision={1} format={(n) => `${n.toFixed(1)}%`} /> : '—'}
          </div>
          <Sparkline data={hist.err ?? []} color="var(--err)" />
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">
            <span>P50 Latency<InfoTip text="Median response latency — half of requests were faster than this." /></span>
            <DeltaTag series={hist.p50} goodWhenUp={false} />
          </div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.p50_latency_ms ?? 0} format={(n) => `${Math.round(n)}ms`} /> : '—'}
          </div>
          <Sparkline data={hist.p50 ?? []} color="var(--ok)" />
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">
            <span>P95 Latency<InfoTip text="95th-percentile latency — 95% of requests were faster than this. Captures tail slowness." /></span>
            <DeltaTag series={hist.p95} goodWhenUp={false} />
          </div>
          <div className="stat-value">
            {m ? <AnimatedMetric value={m.p95_latency_ms ?? 0} format={(n) => `${Math.round(n)}ms`} /> : '—'}
          </div>
          <Sparkline data={hist.p95 ?? []} color="var(--accent-2)" />
        </AdminSurface>
        <AdminSurface className="stat">
          <div className="stat-label">
            <span>Total Requests</span>
            <span className="stat-delta" style={{ color: 'var(--text-3)' }}>24h</span>
          </div>
          <div className="stat-value">
            <AnimatedMetric value={m?.total_requests ?? 0} />
          </div>
        </AdminSurface>
      </div>
      <div className="stats-row" style={{ marginBottom: 16 }}>
        <AdminSurface className="stat stat-compact">
          <div className="stat-label">Streams Started</div>
          <div className="stat-value"><AnimatedMetric value={m?.streams_started ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat stat-compact">
          <div className="stat-label">Completed</div>
          <div className="stat-value ok"><AnimatedMetric value={m?.streams_completed ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat stat-compact">
          <div className="stat-label">Failed</div>
          <div className="stat-value" style={{ color: 'var(--err)' }}><AnimatedMetric value={m?.streams_failed ?? 0} /></div>
        </AdminSurface>
        <AdminSurface className="stat stat-compact">
          <div className="stat-label">Client Disconnects</div>
          <div className="stat-value" style={{ color: 'var(--warn)' }}><AnimatedMetric value={m?.streams_client_disconnected ?? 0} /></div>
        </AdminSurface>
      </div>
      {(m?.pxpipe_compressed_total ?? 0) > 0 && (
        <div className="stats-row" style={{ marginBottom: 16 }}>
          <AdminSurface className="stat stat-compact">
            <div className="stat-label">Image-Compressed Requests</div>
            <div className="stat-value"><AnimatedMetric value={m?.pxpipe_compressed_total ?? 0} /></div>
          </AdminSurface>
          <AdminSurface className="stat stat-compact">
            <div className="stat-label">Images Emitted</div>
            <div className="stat-value"><AnimatedMetric value={m?.pxpipe_images_total ?? 0} /></div>
          </AdminSurface>
          <AdminSurface className="stat stat-compact">
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
