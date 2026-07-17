import type { CSSProperties, ReactNode } from 'react'
import { useStatus } from '../../api/queries'

// Warning box styled like GettingStartedNotice / .settings-restart-banner.
const box = (accent: string): CSSProperties => ({
  padding: '10px 16px',
  border: '1px solid var(--border)',
  borderLeft: `3px solid ${accent}`,
  borderRadius: 'var(--r)',
  fontSize: 13,
  marginBottom: 12,
})

/**
 * Site-wide warning banners rendered above every tab. Reflects live state
 * (cleared automatically when fixed): proxy auth is open (open_relay) or unset
 * (loopback_only). useStatus is React Query-cached, so no extra fetch.
 *
 * Deliberately does NOT warn on an empty Models tab: that lists only
 * model-router deployments (virtual model aliases), which are optional. A
 * configured backend (env or managed) or provider already gives requests a
 * target, so "no router entry" is not an unusable state.
 */
export default function AppBanner() {
  const { data: status } = useStatus(true)

  const banners: ReactNode[] = []

  if (status?.auth_mode === 'open_relay') {
    banners.push(
      <div key="auth" style={box('var(--err)')}>
        <strong>No API key set.</strong> The proxy accepts any request on all
        interfaces (<span className="mono">PROXY_OPEN_RELAY</span>). Anyone who can
        reach this port can spend your provider tokens. Set{' '}
        <span className="mono">PROXY_API_KEYS</span> to require a key.
      </div>,
    )
  } else if (status?.auth_mode === 'loopback_only') {
    banners.push(
      <div key="auth" style={box('var(--warn)')}>
        <strong>No API key set.</strong> The proxy is open on localhost only;
        LAN/remote requests are rejected. Set{' '}
        <span className="mono">PROXY_API_KEYS</span> to require a key for remote
        access.
      </div>,
    )
  }

  if (banners.length === 0) return null
  return <div>{banners}</div>
}
