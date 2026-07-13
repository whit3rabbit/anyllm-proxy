import { create } from 'zustand'

/** Represents the state and actions for admin authentication. */
interface AuthState {
  /** The current authentication token (admin Bearer token) or null if logged out. */
  token: string | null
  /** Performs login by storing and setting the token. */
  login: (token: string) => void
  /** Performs logout by removing the stored token and clearing state. */
  logout: () => void
}

const STORAGE_KEY = 'anyllm_admin_token'

// Pull a `?token=` value out of the URL (the ready-to-click admin URL printed
// at startup on loopback binds), then strip it so it doesn't linger in
// scrollback / bookmarks / history. Returns null when absent.
function takeTokenFromUrl(): string | null {
  try {
    const params = new URLSearchParams(window.location.search)
    const token = params.get('token')
    if (!token) return null
    params.delete('token')
    const search = params.toString()
    const url = window.location.pathname + (search ? `?${search}` : '') + window.location.hash
    window.history.replaceState(null, '', url)
    return token
  } catch {
    return null
  }
}

function getStoredToken(): string | null {
  const fromUrl = takeTokenFromUrl()
  if (fromUrl) {
    setStoredToken(fromUrl)
    return fromUrl
  }
  try {
    return window.sessionStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

function setStoredToken(token: string) {
  try {
    // Session storage is origin-scoped and avoids leaking the admin token to
    // sibling localhost ports via a shared Path=/admin cookie.
    window.sessionStorage.setItem(STORAGE_KEY, token)
  } catch {
    // Zustand still keeps the token for the current page lifetime.
  }
}

function deleteStoredToken() {
  try {
    window.sessionStorage.removeItem(STORAGE_KEY)
  } catch {
    // Nothing to clear when storage is unavailable.
  }
}

/** Zustand store hook for managing admin auth token state. */
export const useAuthStore = create<AuthState>((set) => ({
  token: getStoredToken(),
  login(token) {
    setStoredToken(token)
    set({ token })
  },
  logout() {
    deleteStoredToken()
    set({ token: null })
  },
}))
