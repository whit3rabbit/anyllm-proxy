import type { ProxyUptimeInfo } from '../../api/types'

function formatDuration(startedAt: number) {
  const secs = Math.floor(Date.now() / 1000 - startedAt)
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return `${d}d ${h}h ${m}m`
  if (h > 0) return `${h}h ${m}m`
  return `${m}m`
}

export default function ProxyHealth({ proxy }: { proxy: ProxyUptimeInfo }) {
  return (
    <div className="uptime-proxy">
      <div className="uptime-proxy-stats">
        <div>
          <div className="section-label">Uptime (30d)</div>
          <div className="uptime-pct">{proxy.uptime_pct_30d.toFixed(2)}%</div>
        </div>
        <div>
          <div className="section-label">Running</div>
          <div className="stat-value" style={{ fontSize: 16 }}>{formatDuration(proxy.started_at)}</div>
        </div>
      </div>
      <div className="section-label" style={{ marginBottom: 4 }}>30-day history</div>
      <div className="history-bar">
        {proxy.history.map((day) => (
          <div
            key={day.date}
            className={`history-day ${day.status}`}
            title={`${day.date}: ${day.status}`}
          />
        ))}
      </div>
    </div>
  )
}
