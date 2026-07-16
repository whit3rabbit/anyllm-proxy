import { useSearchParams } from 'react-router-dom'
import Router from '../router/Router'
import RoutesTab from '../routes/Routes'

// Two orthogonal routing mechanisms live under one tab, split by subtab:
//   auto   -> Auto Router: routes each request to a backend+model by request shape.
//   routes -> Model Routes: named model aliases, load-balanced across backends.
const SUBTABS = [
  { key: 'auto', label: 'Auto Router' },
  { key: 'routes', label: 'Model Routes' },
] as const

type SubtabKey = (typeof SUBTABS)[number]['key']

export default function Routing() {
  const [params, setParams] = useSearchParams()
  const active: SubtabKey = params.get('tab') === 'routes' ? 'routes' : 'auto'

  return (
    <div>
      <div className="section-header">
        <h2>Routing</h2>
      </div>
      <div className="mode-toggle" style={{ marginBottom: 16 }}>
        {SUBTABS.map((t) => (
          <button
            key={t.key}
            type="button"
            aria-pressed={active === t.key}
            className={`mode-toggle-btn${active === t.key ? ' active' : ''}`}
            onClick={() => setParams({ tab: t.key })}
          >
            {t.label}
          </button>
        ))}
      </div>
      {active === 'auto' ? <Router /> : <RoutesTab />}
    </div>
  )
}
