type DotStatus = 'ok' | 'warn' | 'err' | 'dim'

const COLOR: Record<DotStatus, string> = {
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  err: 'var(--err)',
  dim: 'var(--text-3)',
}

interface StatusDotProps {
  status: DotStatus
  pulse?: boolean
}

export default function StatusDot({ status, pulse }: StatusDotProps) {
  return (
    <span
      style={{
        display: 'inline-block',
        width: 7,
        height: 7,
        borderRadius: '50%',
        background: COLOR[status],
        animation: pulse ? 'pulse 2s ease-in-out infinite' : undefined,
        verticalAlign: 'middle',
        marginRight: 6,
      }}
    />
  )
}
