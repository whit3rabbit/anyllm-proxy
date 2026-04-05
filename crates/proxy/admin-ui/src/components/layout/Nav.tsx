import { useAuthStore } from '../../store/auth'
import { useWsStore } from '../../store/ws'

type Tab = 'dashboard' | 'requests' | 'settings' | 'backends' | 'keys' | 'models' | 'audit' | 'traffic' | 'uptime'

const TABS: { id: Tab; label: string }[] = [
  { id: 'dashboard', label: 'Dashboard' },
  { id: 'requests', label: 'Request Log' },
  { id: 'settings', label: 'Settings' },
  { id: 'backends', label: 'Backends' },
  { id: 'keys', label: 'Access Control' },
  { id: 'models', label: 'Models' },
  { id: 'audit', label: 'Audit' },
  { id: 'traffic', label: 'Traffic' },
  { id: 'uptime', label: 'Uptime' },
]

interface NavProps {
  activeTab: Tab
  onTabChange: (tab: Tab) => void
}

export default function Nav({ activeTab, onTabChange }: NavProps) {
  const logout = useAuthStore((s) => s.logout)
  const wsStatus = useWsStore((s) => s.status)

  return (
    <nav className="nav">
      <div className="nav-brand">anyllm</div>
      {TABS.map((t) => (
        <div
          key={t.id}
          className={`nav-item${activeTab === t.id ? ' active' : ''}`}
          onClick={() => onTabChange(t.id)}
        >
          {t.label}
        </div>
      ))}
      <div className="nav-right">
        <span
          className={`ws-status ${wsStatus === 'connected' ? 'connected' : 'disconnected'}`}
        >
          {wsStatus === 'connected' ? 'Live' : 'Offline'}
        </span>
        <button
          className="btn btn-secondary btn-sm"
          style={{ marginLeft: 12 }}
          onClick={logout}
        >
          Sign out
        </button>
      </div>
    </nav>
  )
}
