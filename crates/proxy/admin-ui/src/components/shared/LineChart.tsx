interface Series {
  label: string
  color: string
  data: number[]
  secondary?: boolean
}

interface LineChartProps {
  series: Series[]
  labels?: string[]
  gridColor?: string
  height?: number
}

export default function LineChart({
  series,
  labels: _labels,
  gridColor = 'var(--border-sub)',
  height = 130,
}: LineChartProps) {
  const W = 600
  const H = height
  const PAD = { top: 8, right: 8, bottom: 0, left: 0 }
  const innerW = W - PAD.left - PAD.right
  const innerH = H - PAD.top - PAD.bottom

  const allValues = series.flatMap((s) => s.data)
  const maxVal = Math.max(...allValues, 1)
  const len = Math.max(...series.map((s) => s.data.length), 2)

  function x(i: number) {
    return PAD.left + (i / (len - 1)) * innerW
  }
  function y(v: number) {
    return PAD.top + innerH - (v / maxVal) * innerH
  }

  const GRID_LINES = 4
  const gridYs = Array.from({ length: GRID_LINES }, (_, i) =>
    PAD.top + (i / (GRID_LINES - 1)) * innerH,
  )

  return (
    <svg
      className="chart-svg"
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      style={{ height }}
    >
      {gridYs.map((gy, i) => (
        <line
          key={i}
          className="chart-grid-line"
          x1={PAD.left}
          y1={gy}
          x2={W - PAD.right}
          y2={gy}
          stroke={gridColor}
        />
      ))}
      {series.map((s, i) => {
        if (s.data.length < 2) return null
        const points = s.data.map((v, i) => `${x(i)},${y(v)}`).join(' ')
        const areaPoints = [
          `${x(0)},${PAD.top + innerH}`,
          ...s.data.map((v, i) => `${x(i)},${y(v)}`),
          `${x(s.data.length - 1)},${PAD.top + innerH}`,
        ].join(' ')
        return (
          <g key={i}>
            <polygon
              className="chart-area"
              points={areaPoints}
              fill={s.color}
            />
            <polyline
              className={`chart-line${s.secondary ? ' secondary' : ''}`}
              points={points}
              stroke={s.color}
            />
          </g>
        )
      })}
    </svg>
  )
}
