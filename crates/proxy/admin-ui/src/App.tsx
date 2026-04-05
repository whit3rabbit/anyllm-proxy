import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useAuthStore } from './store/auth'
import { useWsStore } from './store/ws'
import { connectWs, disconnectWs } from './api/websocket'
import LoginPage from './components/layout/LoginPage'
import Nav from './components/layout/Nav'
import Dashboard from './tabs/dashboard/Dashboard'
import RequestLog from './tabs/requests/RequestLog'
import Settings from './tabs/settings/Settings'
import Backends from './tabs/backends/Backends'
import Keys from './tabs/keys/Keys'
import Models from './tabs/models/Models'
import Audit from './tabs/audit/Audit'
import TrafficView from './tabs/traffic/TrafficView'
import UptimeView from './tabs/uptime/UptimeView'

type Tab = 'dashboard' | 'requests' | 'settings' | 'backends' | 'keys' | 'models' | 'audit' | 'traffic' | 'uptime'

export default function App() {
  const token = useAuthStore((s) => s.token)
  const login = useAuthStore((s) => s.login)
  const lastEvent = useWsStore((s) => s.lastEvent)
  const qc = useQueryClient()
  const [activeTab, setActiveTab] = useState<Tab>('dashboard')
  const [bootstrapping, setBootstrapping] = useState(true)

  // On mount: if ?token= is in the URL and no token is stored, validate and log in.
  useEffect(() => {
    const params = new URLSearchParams(location.search)
    const urlToken = params.get('token')
    if (urlToken && !token) {
      fetch('/admin/api/metrics', {
        headers: { Authorization: `Bearer ${urlToken}` },
      })
        .then((res) => {
          if (res.ok) {
            login(urlToken)
            history.replaceState(null, '', location.pathname)
          }
        })
        .catch(() => {})
        .finally(() => setBootstrapping(false))
    } else {
      setBootstrapping(false)
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps -- intentionally runs once on mount

  useEffect(() => {
    if (token) {
      connectWs()
    } else {
      disconnectWs()
    }
  }, [token])

  // Invalidate query cache on relevant WS events.
  useEffect(() => {
    if (!lastEvent) return
    if (lastEvent.type === 'metrics_snapshot') {
      qc.setQueryData(['metrics'], lastEvent.data)
    } else if (lastEvent.type === 'backend_health_changed') {
      qc.invalidateQueries({ queryKey: ['uptime'] })
    }
  }, [lastEvent, qc])

  if (bootstrapping) return null
  if (!token) return <LoginPage />

  return (
    <div>
      <Nav activeTab={activeTab} onTabChange={setActiveTab} />
      <div className="tab-content">
        {activeTab === 'dashboard' && <Dashboard />}
        {activeTab === 'requests' && <RequestLog />}
        {activeTab === 'settings' && <Settings />}
        {activeTab === 'backends' && <Backends />}
        {activeTab === 'keys' && <Keys />}
        {activeTab === 'models' && <Models />}
        {activeTab === 'audit' && <Audit />}
        {activeTab === 'traffic' && <TrafficView />}
        {activeTab === 'uptime' && <UptimeView />}
      </div>
    </div>
  )
}
