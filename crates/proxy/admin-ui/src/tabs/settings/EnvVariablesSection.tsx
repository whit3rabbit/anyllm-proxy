import { Fragment } from 'react'

interface EnvVariablesSectionProps {
  envData: Record<string, string> | undefined
}

export default function EnvVariablesSection({ envData }: EnvVariablesSectionProps) {
  if (!envData) return null

  return (
    <div className="readonly-section" style={{ marginTop: 16 }}>
      <div className="section-label">Environment</div>
      <div style={{ display: 'grid', gridTemplateColumns: '220px 1fr', gap: '4px 12px', marginTop: 8, fontSize: 12 }}>
        {Object.entries(envData).map(([k, v]) => (
          <Fragment key={k}>
            <span className="dim">{k}</span>
            <span className="mono">{v}</span>
          </Fragment>
        ))}
      </div>
    </div>
  )
}
