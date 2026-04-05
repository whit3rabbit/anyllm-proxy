interface BudgetBarProps {
  spent: number
  limit: number | null
}

export default function BudgetBar({ spent, limit }: BudgetBarProps) {
  if (!limit) return <span className="dim">—</span>
  const pct = Math.min((spent / limit) * 100, 100)
  const cls = pct >= 95 ? 'danger' : pct >= 80 ? 'warn' : ''
  return (
    <div>
      <div className="budget-bar">
        <div className={`budget-bar-fill${cls ? ` ${cls}` : ''}`} style={{ width: `${pct}%` }} />
      </div>
      <span className="dim" style={{ fontSize: 10 }}>
        ${spent.toFixed(4)} / ${limit.toFixed(2)}
      </span>
    </div>
  )
}
