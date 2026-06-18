import { AdminLoading } from './Performative'

interface EmptyStateProps {
  loading?: boolean
  error?: string | null
  empty?: boolean
  message?: string
}

export default function EmptyState({ loading, error, empty, message }: EmptyStateProps) {
  if (loading) return <div className="empty"><AdminLoading /></div>
  if (error) return <div className="empty error">{error}</div>
  if (empty) return <div className="empty">{message ?? 'No data'}</div>
  return null
}
