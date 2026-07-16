import { useEffect, useRef, useState } from 'react'

/**
 * Accumulates a rolling client-side history for a set of derived metric values.
 *
 * The metrics endpoint returns point-in-time snapshots, not a series — this
 * hook remembers the last `max` snapshots so stat cards can draw sparklines and
 * trend deltas. History resets on reload (it is purely presentational); the
 * operator charts on the dashboard use the real server-side `observability`
 * series instead.
 *
 * `select` maps a snapshot to the numeric values to track. It is only invoked
 * when the snapshot object identity changes (react-query keeps it stable
 * between refetches), so passing an inline function is fine.
 */
export function useMetricHistory<T>(
  snapshot: T | undefined,
  select: (s: T) => Record<string, number>,
  max = 24,
): Record<string, number[]> {
  const ref = useRef<Record<string, number[]>>({})
  const [, force] = useState(0)

  useEffect(() => {
    if (!snapshot) return
    const values = select(snapshot)
    for (const key of Object.keys(values)) {
      const prev = ref.current[key] ?? []
      ref.current[key] = [...prev, values[key]].slice(-max)
    }
    force((n) => n + 1)
    // Intentionally keyed on snapshot identity only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot])

  return ref.current
}

/** Percentage change from the first to the last point of a series. */
export function trendDelta(series: number[] | undefined): number | null {
  if (!series || series.length < 2) return null
  const first = series[0]
  const last = series[series.length - 1]
  if (first === 0) return null
  return ((last - first) / Math.abs(first)) * 100
}
