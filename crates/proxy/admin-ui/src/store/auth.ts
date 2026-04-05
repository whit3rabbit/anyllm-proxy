import { create } from 'zustand'

interface AuthState {
  token: string | null
  login: (token: string) => void
  logout: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  token: sessionStorage.getItem('admin_token'),
  login(token) {
    sessionStorage.setItem('admin_token', token)
    set({ token })
  },
  logout() {
    sessionStorage.removeItem('admin_token')
    set({ token: null })
  },
}))
