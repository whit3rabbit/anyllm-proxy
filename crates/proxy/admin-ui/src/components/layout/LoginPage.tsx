import { useState, type FormEvent } from 'react'
import { useAuthStore } from '../../store/auth'

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
      <div className="login-card">
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
          <button type="submit" className="btn btn-primary" disabled={loading}>
            {loading ? 'Signing in\u2026' : 'Sign in'}
          </button>
        </form>
        <div className="login-error">{error}</div>
      </div>
    </div>
  )
}
