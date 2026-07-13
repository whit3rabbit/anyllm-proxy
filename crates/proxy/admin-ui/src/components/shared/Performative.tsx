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

/** Props for the AdminButton component. */
export interface AdminButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** The button style tone (primary, secondary, danger, icon). */
  tone?: ButtonTone
  /** The size of the button (sm, md). */
  size?: ButtonSizeName
  /** Whether the button should show a loading spinner. */
  loading?: boolean
  /** Whether the button should stretch to fill the width of its container. */
  block?: boolean
  /** Inner content. */
  children: ReactNode
}

/** Wrapper component around performative-ui's Button, providing theme colors and states. */
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

/** Props for the AdminStatusDot component. */
interface AdminStatusDotProps {
  /** Status indicator state ('ok', 'warn', 'err', 'dim'). */
  status: DotStatus
  /** Whether the status dot should pulse/animate. */
  pulse?: boolean
}

/** Renders a styled status indicator dot using performative-ui. */
export function AdminStatusDot({ status, pulse }: AdminStatusDotProps) {
  return (
    <PuiStatusDot
      color={DOT_COLOR[status]}
      static={!pulse}
      className="admin-status-dot"
    />
  )
}

/** Props for the AdminSurface container component. */
interface AdminSurfaceProps extends ComponentPropsWithoutRef<'article'> {
  /** Enables a subtle breathing opacity animation. */
  breathing?: boolean
  /** Enables a hover glow styling effect. */
  glowOnHover?: boolean
}

/** A container surface card with optional glow and breathing animations. */
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

/** Props for the AdminLoading indicator component. */
interface AdminLoadingProps {
  /** Primary loading message/verb to display. */
  label?: string
  /** Optional secondary info nodes or helper text. */
  info?: ReactNode
  /** Additional styling class names. */
  className?: string
}

/** A styled loading spinner component displaying animated messages. */
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

/** Props for the AnimatedMetric counter component. */
interface AnimatedMetricProps {
  /** Target numeric value to count up/down to. */
  value: number
  /** Number of decimal places to round to. */
  precision?: number
  /** Duration of the counting animation in milliseconds. */
  durationMs?: number
  /** Additional styling class names. */
  className?: string
  /** Custom formatter function for the displayed value. */
  format?: (value: number) => string
}

/** Renders a numeric value with a smooth count-up animation on value change. */
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
