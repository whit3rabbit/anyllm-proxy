import { useState } from 'react'
import { useTraffic } from '../../api/queries'
import RouteTable from './RouteTable'
import LineChart from '../../components/shared/LineChart'
import EmptyState from '../../components/shared/EmptyState'

const COLORS = ['#e8a030', '#4caf6e', '#6eb5c0', '#c87dd4', '#e05252']

export default function TrafficView() {
  const [windowHours, setWindowHours] = useState(6)
  const { data, isLoading, error } = useTraffic(windowHours)

  const routes = data?.routes ?? []
  const series = routes.slice(0, 5).map((r, i) => {
    const points = (data?.series ?? [])
      .filter((p) => p.path === r.path)
      .map((p) => p.requests)
    return { label: r.path, color: COLORS[i % COLORS.length], data: points }
  })

  const payloadSeries = routes.slice(0, 5).map((r, i) => ({
    label: r.path,
    color: COLORS[i % COLORS.length],
    data: [r.avg_request_bytes],
  }))

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
              <LineChart series={payloadSeries} />
            </div>
          </div>
        </>
      )}
    </div>
  )
}
