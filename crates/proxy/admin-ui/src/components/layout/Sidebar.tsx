import { NavLink } from 'react-router-dom'
import { useAuthStore } from '../../store/auth'
import { useWsStore } from '../../store/ws'

interface SidebarGroup {
  label: string
  items: { to: string; label: string }[]
}

const GROUPS: SidebarGroup[] = [
  {
    label: 'Overview',
    items: [
      { to: '/dashboard', label: 'Dashboard' },
      { to: '/requests', label: 'Request Log' },
      { to: '/traffic', label: 'Traffic' },
    ],
  },
  {
    label: 'Configure',
    items: [
      { to: '/providers', label: 'Providers' },
      { to: '/routes', label: 'Routes' },
      { to: '/models', label: 'Models' },
    ],
  },
  {
    label: 'Access',
    items: [
      { to: '/keys', label: 'API Keys' },
      { to: '/audit', label: 'Audit Log' },
    ],
  },
  {
    label: 'System',
    items: [
      { to: '/settings', label: 'Settings' },
      { to: '/uptime', label: 'Uptime' },
    ],
  },
]

export default function Sidebar() {
  const logout = useAuthStore((s) => s.logout)
  const wsStatus = useWsStore((s) => s.status)

  return (
    <aside className="sidebar">
      <div className="sidebar-brand">anyllm</div>

      <div className="sidebar-scroll">
        {GROUPS.map((g) => (
          <div key={g.label} className="sidebar-group">
            <div className="sidebar-group-label">{g.label}</div>
            {g.items.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) => `sidebar-item${isActive ? ' active' : ''}`}
              >
                {item.label}
              </NavLink>
            ))}
          </div>
        ))}
      </div>

      <div className="sidebar-footer">
        <span className={`ws-status ${wsStatus === 'connected' ? 'connected' : 'disconnected'}`}>
          {wsStatus === 'connected' ? 'Live' : 'Offline'}
        </span>
        <button className="btn btn-secondary btn-sm" onClick={logout}>
          Sign out
        </button>
      </div>
    </aside>
  )
}
