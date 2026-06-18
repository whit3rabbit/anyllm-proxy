import { useState, type FormEvent } from 'react'
import { NodeGraphBackground } from 'performative-ui'
import { useAuthStore } from '../../store/auth'
import { AdminButton, AdminSurface } from '../shared/Performative'

export default function LoginPage() {
  const login = useAuthStore((s) => s.login)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  async function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault()
    const token = (e.currentTarget.elements.namedItem('token') as HTMLInputElement).value.trim()
    if (!token) return
    setLoading(true)
    setError('')
    try {
      // Validate by hitting a lightweight endpoint with the candidate token.
      const res = await fetch('/admin/api/metrics', {
        headers: { Authorization: `Bearer ${token}` },
      })
      if (!res.ok) throw new Error('Invalid token')
      login(token)
    } catch {
      setError('Invalid token')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="login-overlay">
      <NodeGraphBackground
        density={24}
        speed={0.16}
        linkDistance={110}
        hoverDistance={120}
        hoverGravity={0.002}
        baseOpacity={0.18}
        colors={['#e8a030', '#4caf6e', '#5aa9e6']}
        linkColor="#e8a030"
        className="login-node-bg"
      />
      <AdminSurface className="login-card" glowOnHover breathing>
        <div className="login-title">
          <span className="prompt">&gt;&nbsp;</span>proxy admin
        </div>
        <form onSubmit={handleSubmit}>
          <input
            type="password"
            name="token"
            placeholder="Admin token"
            autoComplete="current-password"
            autoFocus
          />
          <AdminButton type="submit" tone="primary" loading={loading} block>
            Sign in
          </AdminButton>
        </form>
        <div className="login-error">{error}</div>
      </AdminSurface>
    </div>
  )
}
