import { create } from 'zustand'

interface AuthState {
  token: string | null
  login: (token: string) => void
  logout: () => void
}

const STORAGE_KEY = 'anyllm_admin_token'

function getStoredToken(): string | null {
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
