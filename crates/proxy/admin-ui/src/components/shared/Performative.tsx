import {
  useEffect,
  useRef,
  type ButtonHTMLAttributes,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from 'react'
import {
  Button,
  GlassCard,
  StatCounter,
  StatusDot as PuiStatusDot,
  WibblingSpinner,
  type ButtonSize,
  type ButtonVariant,
} from 'performative-ui'

type ButtonTone = 'primary' | 'secondary' | 'danger' | 'icon'
type ButtonSizeName = 'sm' | 'md'

const BUTTON_VARIANT: Record<ButtonTone, ButtonVariant> = {
  primary: 'wave',
  secondary: 'ghost',
  danger: 'ghost',
  icon: 'ghost',
}

const BUTTON_SIZE: Record<ButtonSizeName, ButtonSize> = {
  sm: 'sm',
  md: 'md',
}

export interface AdminButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  tone?: ButtonTone
  size?: ButtonSizeName
  loading?: boolean
  block?: boolean
  children: ReactNode
}

export function AdminButton({
  tone = 'secondary',
  size = 'md',
  loading = false,
  block = false,
  className,
  children,
  disabled,
  ...props
}: AdminButtonProps) {
  return (
    <Button
      variant={BUTTON_VARIANT[tone]}
      size={BUTTON_SIZE[size]}
      loading={loading}
      block={block}
      className={['admin-button', `admin-button-${tone}`, className].filter(Boolean).join(' ')}
      disabled={disabled || loading}
      {...props}
    >
      {children}
    </Button>
  )
}

type DotStatus = 'ok' | 'warn' | 'err' | 'dim'

const DOT_COLOR: Record<DotStatus, string> = {
  ok: 'var(--ok)',
  warn: 'var(--warn)',
  err: 'var(--err)',
  dim: 'var(--text-3)',
}

interface AdminStatusDotProps {
  status: DotStatus
  pulse?: boolean
}

export function AdminStatusDot({ status, pulse }: AdminStatusDotProps) {
  return (
    <PuiStatusDot
      color={DOT_COLOR[status]}
      static={!pulse}
      className="admin-status-dot"
    />
  )
}

interface AdminSurfaceProps extends ComponentPropsWithoutRef<'article'> {
  breathing?: boolean
  glowOnHover?: boolean
}

export function AdminSurface({
  children,
  className,
  breathing = false,
  glowOnHover = false,
  ...props
}: AdminSurfaceProps) {
  return (
    <GlassCard
      breathing={breathing}
      glowOnHover={glowOnHover}
      className={['admin-surface', className].filter(Boolean).join(' ')}
      {...props}
    >
      {children}
    </GlassCard>
  )
}

interface AdminLoadingProps {
  label?: string
  info?: ReactNode
  className?: string
}

export function AdminLoading({ label = 'Loading', info, className }: AdminLoadingProps) {
  return (
    <WibblingSpinner
      verbs={[label]}
      glyphs={['.', 'o', 'O', 'o']}
      glyphInterval={220}
      ellipsis="..."
      info={info}
      glyphColor="var(--accent)"
      className={['admin-loading', className].filter(Boolean).join(' ')}
    />
  )
}

interface AnimatedMetricProps {
  value: number
  precision?: number
  durationMs?: number
  className?: string
  format?: (value: number) => string
}

export function AnimatedMetric({
  value,
  precision = 0,
  durationMs = 450,
  className,
  format,
}: AnimatedMetricProps) {
  const factor = 10 ** precision
  const target = Math.round(value * factor)
  const previous = useRef(target)
  const from = previous.current

  useEffect(() => {
    previous.current = target
  }, [target])

  return (
    <StatCounter
      key={target}
      target={target}
      from={from}
      durationMs={durationMs}
      className={className}
      format={(n) => {
        const scaled = n / factor
        return format ? format(scaled) : scaled.toLocaleString()
      }}
    />
  )
}
