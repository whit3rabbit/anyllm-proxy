type BadgeVariant = 'active' | 'revoked' | 'expired' | 'override'

export default function Badge({ variant }: { variant: BadgeVariant }) {
  return <span className={`badge badge-${variant}`}>{variant}</span>
}
