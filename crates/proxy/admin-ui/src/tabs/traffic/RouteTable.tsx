import type { RouteMetrics } from '../../api/types'

export default function RouteTable({ routes }: { routes: RouteMetrics[] }) {
  const sorted = [...routes].sort((a, b) => b.requests_per_min - a.requests_per_min)
  return (
    <table className="route-table">
      <thead>
        <tr>
          <th>Route</th>
          <th>Req/min</th>
          <th>Error rate</th>
          <th>Avg latency</th>
          <th>P95 latency</th>
          <th>Total</th>
        </tr>
      </thead>
      <tbody>
        {sorted.map((r) => (
          <tr key={r.path}>
            <td className="mono">{r.path}</td>
            <td className="mono">{r.requests_per_min.toFixed(2)}</td>
            <td className="mono" style={{ color: r.error_rate > 0.05 ? 'var(--err)' : r.error_rate > 0.01 ? 'var(--warn)' : undefined }}>
              {(r.error_rate * 100).toFixed(1)}%
            </td>
            <td className="mono">{r.avg_latency_ms.toFixed(0)}ms</td>
            <td className="mono">{r.p95_latency_ms}ms</td>
            <td className="mono">{r.total_requests.toLocaleString()}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

