import type { BackendUptimeInfo } from '../../api/types'
import StatusDot from '../../components/shared/StatusDot'

export default function BackendHealthRow({ b }: { b: BackendUptimeInfo }) {
  const dotStatus: 'ok' | 'err' | 'dim' = b.status === 'up' ? 'ok' : b.status === 'down' ? 'err' : 'dim'
  const lastChecked = b.last_checked_at
    ? new Date(b.last_checked_at * 1000).toLocaleTimeString()
    : '—'

  return (
    <tr>
      <td className="mono">{b.name}</td>
      <td>
        <StatusDot status={dotStatus} pulse={b.status === 'up'} />
        {b.status}
      </td>
      <td className="mono">{b.uptime_pct_30d.toFixed(2)}%</td>
      <td className="mono dim">{lastChecked}</td>
      <td className="mono dim">{b.last_latency_ms != null ? `${b.last_latency_ms}ms` : '—'}</td>
      <td>
        <div className="history-bar" style={{ height: 12 }}>
          {b.history.map((day) => (
            <div
              key={day.date}
              className={`history-day ${day.status}`}
              title={`${day.date}: ${day.status}`}
            />
          ))}
        </div>
      </td>
    </tr>
  )
}
