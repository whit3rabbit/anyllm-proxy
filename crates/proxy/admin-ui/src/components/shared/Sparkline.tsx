/**
 * Minimal dependency-free SVG sparkline. Renders a filled area + line from a
 * numeric series, scaled to its own min/max. Stroke width is constant thanks to
 * `vector-effect: non-scaling-stroke`, so it stays crisp at any card width.
 */
interface SparklineProps {
  data: number[]
  color?: string
  height?: number
  fillOpacity?: number
}

const W = 120

export default function Sparkline({
  data,
  color = 'var(--accent)',
  height = 30,
  fillOpacity = 0.1,
}: SparklineProps) {
  const H = height
  if (!data || data.length < 2) {
    return <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ width: '100%', height: H, display: 'block' }} />
  }

  const max = Math.max(...data)
  const min = Math.min(...data)
  const range = max - min || 1
  const x = (i: number) => (i / (data.length - 1)) * W
  const y = (v: number) => H - ((v - min) / range) * (H - 4) - 2

  const line = data.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')
  const area = `0,${H} ${line} ${W},${H}`

  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ width: '100%', height: H, display: 'block' }}>
      <polyline points={area} fill={color} fillOpacity={fillOpacity} stroke="none" />
      <polyline
        points={line}
        fill="none"
        stroke={color}
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  )
}
