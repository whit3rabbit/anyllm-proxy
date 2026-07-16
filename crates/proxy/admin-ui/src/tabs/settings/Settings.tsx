import { useConfig, useEnv, useStatus } from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'
import GettingStartedNotice from './GettingStartedNotice'
import EnvFileSection from './EnvFileSection'
import DisplaySection from './DisplaySection'
import RuntimeSettingsSection from './RuntimeSettingsSection'
import EnvVariablesSection from './EnvVariablesSection'

/**
 * Settings Component.
 * Coordinates and provides controls for configuring system settings,
 * environment variables export/import, and proxy properties.
 */
export default function Settings({ configured = true }: { configured?: boolean }) {
  const { data: cfg, isLoading, error } = useConfig()
  const { data: envData } = useEnv()
  const { data: status } = useStatus()

  const proxyUrl = status ? `http://${window.location.hostname}:${status.proxy_port}` : ''

  return (
    <div>
      {/* Live proxy status. Every setting below applies immediately (no restart);
          this badge just confirms the proxy port is actually reachable. */}
      {status && (
        <div
          className="proxy-status-badge"
          style={{ marginBottom: 16, fontSize: 13, display: 'flex', alignItems: 'center', gap: 8 }}
        >
          <span style={{ color: status.proxy_running ? 'var(--ok, green)' : 'var(--warn, orange)' }}>
            {status.proxy_running ? '●' : '○'}
          </span>
          {status.proxy_running
            ? <span>Proxy running — <span className="mono">{proxyUrl}</span></span>
            : <span>Proxy unreachable on <span className="mono">{proxyUrl}</span></span>}
        </div>
      )}

      {/* Getting-started notice — shown when no backend is configured */}
      <GettingStartedNotice configured={configured} />

      {/* Display: accent color, mode, text size (client-side only) */}
      <DisplaySection />

      {/* Env file import / export */}
      <EnvFileSection />

      <EmptyState loading={isLoading} error={error?.message} />

      {/* Runtime settings override form */}
      {cfg && <RuntimeSettingsSection cfg={cfg} />}

      {/* Environment grid */}
      <EnvVariablesSection envData={envData} />
    </div>
  )
}
