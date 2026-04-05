import { useState } from 'react'
import { useTraffic } from '../../api/queries'
import RouteTable from './RouteTable'
import LineChart from '../../components/shared/LineChart'
import EmptyState from '../../components/shared/EmptyState'

// Amber palette for requests/min chart
const AMBER_COLORS = ['#e8a030', '#d4922b', '#c07820', '#a86015', '#8c500a']
// Teal palette for payload bar chart
const TEAL_COLORS = ['#6eb5c0', '#5aa0ab', '#468b96', '#327681', '#1e616c']

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export default function TrafficView() {
  const [windowHours, setWindowHours] = useState(6)
  const { data, isLoading, error } = useTraffic(windowHours)

  const routes = data?.routes ?? []
  const series = routes.slice(0, 5).map((r, i) => {
    const points = (data?.series ?? [])
      .filter((p) => p.path === r.path)
      .map((p) => p.requests)
    return { label: r.path, color: AMBER_COLORS[i % AMBER_COLORS.length], data: points }
  })

  return (
    <div>
      <div className="section-header">
        <span className="section-label">Traffic</span>
        <select value={windowHours} onChange={(e) => setWindowHours(Number(e.target.value))}>
          <option value={1}>Last 1 hour</option>
          <option value={6}>Last 6 hours</option>
          <option value={24}>Last 24 hours</option>
        </select>
      </div>

      <EmptyState loading={isLoading} error={error?.message} />

      {data && (
        <>
          <RouteTable routes={data.routes} />

          <div className="operator-grid" style={{ marginTop: 16 }}>
            <div className="chart-card">
              <div className="chart-header">
                <div>
                  <div className="chart-title">Requests / min by route</div>
                  <div className="chart-subtitle">Stacked over time window</div>
                </div>
              </div>
              <LineChart series={series} />
            </div>
            <div className="chart-card">
              <div className="chart-header">
                <div>
                  <div className="chart-title">Avg payload per route</div>
                  <div className="chart-subtitle">Bytes</div>
                </div>
              </div>
              {routes.length === 0 ? (
                <div className="empty">No routes</div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 8 }}>
                  {routes.slice(0, 5).map((r, i) => {
                    const maxBytes = Math.max(...routes.slice(0, 5).map((x) => x.avg_request_bytes), 1)
                    const pct = (r.avg_request_bytes / maxBytes) * 100
                    return (
                      <div key={r.path}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginBottom: 2 }}>
                          <span className="mono dim" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: '70%' }}>{r.path}</span>
                          <span className="mono">{formatBytes(r.avg_request_bytes)}</span>
                        </div>
                        <div style={{ height: 6, background: 'var(--border)', borderRadius: 0 }}>
                          <div style={{ height: '100%', width: `${pct}%`, background: TEAL_COLORS[i % TEAL_COLORS.length], borderRadius: 0 }} />
                        </div>
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  )
}
