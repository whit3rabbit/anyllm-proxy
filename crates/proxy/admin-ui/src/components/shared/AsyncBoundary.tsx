import type { ReactNode } from 'react'
import type { UseQueryResult } from '@tanstack/react-query'

interface AsyncBoundaryProps<T> {
  query: UseQueryResult<T, Error>
  children: (data: T) => ReactNode
  /** Predicate + render for the "loaded but empty" state. */
  empty?: { when: (data: T) => boolean; render: () => ReactNode }
  /** Custom loading UI. Defaults to <SkeletonRows>. */
  loading?: ReactNode
  errorTitle?: string
  /** Number of skeleton rows in the default loading UI. */
  skeletonRows?: number
}

export default function AsyncBoundary<T>({
  query,
  children,
  empty,
  loading,
  errorTitle = 'Failed to load',
  skeletonRows = 3,
}: AsyncBoundaryProps<T>) {
  if (query.isLoading && query.data === undefined) {
    return <>{loading ?? <SkeletonRows count={skeletonRows} />}</>
  }
  if (query.isError) {
    return (
      <div className="async-error" role="alert">
        <div className="async-error-title">{errorTitle}</div>
        <div className="async-error-message">
          {query.error instanceof Error ? query.error.message : String(query.error)}
        </div>
        <button
          type="button"
          className="btn btn-secondary"
          onClick={() => { void query.refetch() }}
          disabled={query.isFetching}
        >
          {query.isFetching ? 'Retrying…' : 'Retry'}
        </button>
      </div>
    )
  }
  if (query.data === undefined) return null
  if (empty && empty.when(query.data)) return <>{empty.render()}</>
  return <>{children(query.data)}</>
}

function SkeletonRows({ count }: { count: number }) {
  return (
    <div className="skeleton-stack" aria-hidden="true">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="skeleton skeleton-row" />
      ))}
    </div>
  )
}
