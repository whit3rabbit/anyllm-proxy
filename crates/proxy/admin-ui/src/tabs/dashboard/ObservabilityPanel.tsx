import { useState } from 'react'
import { useObservability, useBackends } from '../../api/queries'
import LineChart from '../../components/shared/LineChart'
import EmptyState from '../../components/shared/EmptyState'
import { AdminSurface, AnimatedMetric } from '../../components/shared/Performative'
import InfoTip from '../../components/shared/InfoTip'

export default function ObservabilityPanel() {
  const [windowHours, setWindowHours] = useState(6)
  const [backend, setBackend] = useState('')
  const { data: backends } = useBackends()
  const { data, isLoading, error } = useObservability(windowHours, backend)

  const reqSeries = data
    ? [
        { label: 'Requests', color: '#e8a030', data: data.series.map((p) => p.requests) },
        { label: 'Errors', color: '#e05252', data: data.series.map((p) => p.errors), secondary: true },
      ]
    : []

  const tokenSeries = data
    ? [
        { label: 'Input', color: '#4caf6e', data: data.series.map((p) => p.input_tokens) },
        { label: 'Output', color: '#6eb5c0', data: data.series.map((p) => p.output_tokens), secondary: true },
      ]
    : []

  const costSeries = data
    ? [{ label: 'Cost', color: '#c87dd4', data: data.series.map((p) => p.cost_usd) }]
    : []

  return (
    <div>
      <div className="operator-controls">
        <span className="section-label" style={{ marginBottom: 0 }}>Operator View</span>
        <div className="form-row" style={{ flexWrap: 'wrap', gap: 6, marginTop: 0 }}>
          <select value={windowHours} onChange={(e) => setWindowHours(Number(e.target.value))}>
            <option value={1}>Last 1 hour</option>
            <option value={6}>Last 6 hours</option>
            <option value={24}>Last 24 hours</option>
          </select>
          <select value={backend} onChange={(e) => setBackend(e.target.value)}>
            <option value="">All backends</option>
            {backends?.map((b) => (
              <option key={b.name} value={b.name}>{b.name}</option>
            ))}
          </select>
        </div>
      </div>

      {data && (
        <div className="stats-row">
          <AdminSurface className="stat">
            <div className="stat-label">Input Tokens</div>
            <div className="stat-value"><AnimatedMetric value={data.total_input_tokens} /></div>
          </AdminSurface>
          <AdminSurface className="stat">
            <div className="stat-label">Output Tokens</div>
            <div className="stat-value"><AnimatedMetric value={data.total_output_tokens} /></div>
          </AdminSurface>
          <AdminSurface className="stat">
            <div className="stat-label">Window Failures<InfoTip text="Failed (error) requests within the selected time window and backend filter." /></div>
            <div className="stat-value"><AnimatedMetric value={data.total_errors} /></div>
          </AdminSurface>
          <AdminSurface className="stat">
            <div className="stat-label">Window Cost<InfoTip text="Estimated USD spend within the selected time window and backend filter, from model pricing." /></div>
            <div className="stat-value"><AnimatedMetric value={data.total_cost_usd} precision={2} format={(n) => `$${n.toFixed(2)}`} /></div>
          </AdminSurface>
        </div>
      )}

      <EmptyState loading={isLoading} error={error?.message} />

      {data && (
        <div className="operator-grid">
          <AdminSurface className="chart-card">
            <div className="chart-header">
              <div>
                <div className="chart-title">Request Volume</div>
                <div className="chart-subtitle">Rolling request count and errors</div>
              </div>
              <div className="chart-value">{data.total_requests}</div>
            </div>
            <LineChart series={reqSeries} />
          </AdminSurface>
          <AdminSurface className="chart-card">
            <div className="chart-header">
              <div>
                <div className="chart-title">Tokens</div>
                <div className="chart-subtitle">Input and output usage</div>
              </div>
              <div className="chart-value">{(data.total_input_tokens + data.total_output_tokens).toLocaleString()}</div>
            </div>
            <LineChart series={tokenSeries} />
          </AdminSurface>
          <AdminSurface className="chart-card">
            <div className="chart-header">
              <div>
                <div className="chart-title">Estimated Cost</div>
                <div className="chart-subtitle">USD by minute bucket</div>
              </div>
              <div className="chart-value">${data.total_cost_usd.toFixed(4)}</div>
            </div>
            <LineChart series={costSeries} />
          </AdminSurface>
        </div>
      )}
    </div>
  )
}
