import { create } from 'zustand'

interface AuthState {
  token: string | null
  login: (token: string) => void
  logout: () => void
}

function getCookie(name: string): string | null {
  const m = document.cookie.match(new RegExp('(?:^|; )' + name + '=([^;]*)'))
  return m ? decodeURIComponent(m[1]) : null
}

function setCookie(name: string, value: string) {
  document.cookie = `${name}=${encodeURIComponent(value)}; Path=/admin; SameSite=Strict; Max-Age=604800`
}

function deleteCookie(name: string) {
  document.cookie = `${name}=; Path=/admin; SameSite=Strict; Max-Age=0`
}

export const useAuthStore = create<AuthState>((set) => ({
  token: getCookie('admin_session'),
  login(token) {
    setCookie('admin_session', token)
    set({ token })
  },
  logout() {
    deleteCookie('admin_session')
    set({ token: null })
  },
}))
