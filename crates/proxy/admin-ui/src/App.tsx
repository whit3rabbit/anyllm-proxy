import { useEffect } from 'react'
import { HashRouter, Routes as RouterRoutes, Route, Navigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { useAuthStore } from './store/auth'
import { useWsStore } from './store/ws'
import { connectWs, disconnectWs } from './api/websocket'
import { useStatus } from './api/queries'
import LoginPage from './components/layout/LoginPage'
import Sidebar from './components/layout/Sidebar'
import ToastProvider from './components/shared/Toast'
import AppBanner from './components/shared/AppBanner'
import Dashboard from './tabs/dashboard/Dashboard'
import RequestLog from './tabs/requests/RequestLog'
import Settings from './tabs/settings/Settings'
import Keys from './tabs/keys/Keys'
import Models from './tabs/models/Models'
import Audit from './tabs/audit/Audit'
import TrafficView from './tabs/traffic/TrafficView'
import UptimeView from './tabs/uptime/UptimeView'
import Providers from './tabs/providers/Providers'
import Backends from './tabs/backends/Backends'
import Routing from './tabs/routing/Routing'

/**
 * Root Application component. Sets up routing, global queries,
 * WebSocket connections for real-time logs, and renders the layout.
 */
export default function App() {
  const token = useAuthStore((s) => s.token)
  const lastEvent = useWsStore((s) => s.lastEvent)
  const qc = useQueryClient()
  const { data: status } = useStatus(!!token)

  useEffect(() => {
    if (token) {
      connectWs()
    } else {
      disconnectWs()
    }
  }, [token])

  useEffect(() => {
    if (!lastEvent) return
    if (lastEvent.type === 'metrics_snapshot') {
      qc.setQueryData(['metrics'], lastEvent.data)
    } else if (lastEvent.type === 'backend_health_changed') {
      qc.invalidateQueries({ queryKey: ['uptime'] })
    } else if (lastEvent.type === 'config_changed') {
      // Settings uses staleTime: Infinity; without this a config change from
      // another admin session or the CLI never refreshes the UI.
      qc.invalidateQueries({ queryKey: ['config'] })
      qc.invalidateQueries({ queryKey: ['env'] })
    }
  }, [lastEvent, qc])

  if (!token) {
    return (
      <>
        <LoginPage />
        <ToastProvider />
      </>
    )
  }

  const configured = status?.configured ?? true

  return (
    <HashRouter>
      <div className="app-layout">
        <Sidebar />
        <div className="tab-content">
          <AppBanner />
          <RouterRoutes>
            <Route
              path="/"
              element={<Navigate to={configured ? '/dashboard' : '/settings'} replace />}
            />
            <Route path="/dashboard" element={<Dashboard />} />
            <Route path="/requests" element={<RequestLog />} />
            <Route path="/traffic" element={<TrafficView />} />
            <Route path="/providers" element={<Providers />} />
            <Route path="/routing" element={<Routing />} />
            {/* Back-compat: the old two tabs are now subtabs of /routing. */}
            <Route path="/routes" element={<Navigate to="/routing?tab=routes" replace />} />
            <Route path="/router" element={<Navigate to="/routing?tab=auto" replace />} />
            <Route path="/models" element={<Models />} />
            <Route path="/backends" element={<Backends />} />
            <Route path="/keys" element={<Keys />} />
            <Route path="/audit" element={<Audit />} />
            <Route path="/settings" element={<Settings configured={configured} />} />
            <Route path="/uptime" element={<UptimeView />} />
            <Route path="*" element={<Navigate to="/dashboard" replace />} />
          </RouterRoutes>
        </div>
        <ToastProvider />
      </div>
    </HashRouter>
  )
}
