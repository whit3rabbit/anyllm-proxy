import { NavLink } from 'react-router-dom'
import { useAuthStore } from '../../store/auth'
import { useWsStore } from '../../store/ws'
import { NAV_ICONS } from '../shared/NavIcon'
import { AdminButton } from '../shared/Performative'

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
      { to: '/routing', label: 'Routing' },
      { to: '/models', label: 'Models' },
      { to: '/backends', label: 'Backends' },
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
      <div className="sidebar-brand">
        <div className="sidebar-brand-row">
          <span className="sidebar-brand-dot" />
          anyllm
        </div>
        <span className="sidebar-brand-sub">Proxy Console</span>
      </div>

      <nav className="sidebar-scroll">
        {GROUPS.map((g) => (
          <div key={g.label} className="sidebar-group">
            <div className="sidebar-group-label">{g.label}</div>
            {g.items.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) => `sidebar-item${isActive ? ' active' : ''}`}
              >
                <span className="nav-ico">{NAV_ICONS[item.to]}</span>
                <span>{item.label}</span>
              </NavLink>
            ))}
          </div>
        ))}
      </nav>

      <div className="sidebar-footer">
        <div className="sidebar-footer-row">
          <span className={`ws-status ${wsStatus === 'connected' ? 'connected' : 'disconnected'}`}>
            {wsStatus === 'connected' ? 'Live' : 'Offline'}
          </span>
          <AdminButton size="sm" onClick={logout}>
            Sign out
          </AdminButton>
        </div>
      </div>
    </aside>
  )
}
