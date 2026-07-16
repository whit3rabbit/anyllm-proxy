import { create } from 'zustand'

/**
 * Theme store — two independent axes plus font-scale.
 *
 *  - accent color  (`data-theme` on <html>): amber/blue/emerald/rose/cyan.
 *    Only swaps the accent family.
 *  - mode          (`data-mode`  on <html>): dark (default) or light.
 *    Light swaps the whole neutral ramp; it sits ON TOP of any accent
 *    color, so blue+light, emerald+dark, etc. are all valid combos.
 *  - font scale    (root `font-size`): all UI text is rem so one value
 *    scales the whole app.
 *
 * All three persist to localStorage and apply to the document as this
 * module loads (before React mounts) so there is no flash.
 */

export type Theme = 'amber' | 'blue' | 'emerald' | 'rose' | 'cyan'
export type Mode = 'dark' | 'light'

export const THEMES: { key: Theme; label: string; swatch: string }[] = [
  { key: 'blue', label: 'Blue', swatch: '#4c8dff' },
  { key: 'amber', label: 'Amber', swatch: '#e8a030' },
  { key: 'emerald', label: 'Emerald', swatch: '#2fc98a' },
  { key: 'rose', label: 'Rose', swatch: '#f0518a' },
  { key: 'cyan', label: 'Cyan', swatch: '#22c3d6' },
]

const DEFAULT_THEME: Theme = 'blue'

export const FONT_MIN = 14
export const FONT_MAX = 20

const THEME_KEY = 'anyllm.theme'
const MODE_KEY = 'anyllm.mode'
const FONT_KEY = 'anyllm.fontScale'

function ls(key: string): string | null {
  return typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null
}

function readTheme(): Theme {
  const t = ls(THEME_KEY)
  // Legacy: 'light' used to be a color choice; it's now a mode (see readMode).
  if (t === 'light') return DEFAULT_THEME
  return t && THEMES.some((x) => x.key === t) ? (t as Theme) : DEFAULT_THEME
}
function readMode(): Mode {
  const m = ls(MODE_KEY)
  if (m === 'dark' || m === 'light') return m
  // Legacy migration: old theme='light' implied light mode.
  return ls(THEME_KEY) === 'light' ? 'light' : 'dark'
}
function readFont(): number {
  const n = Number(ls(FONT_KEY))
  return n >= FONT_MIN && n <= FONT_MAX ? n : 16
}

function applyTheme(theme: Theme) {
  document.documentElement.setAttribute('data-theme', theme)
}
function applyMode(mode: Mode) {
  document.documentElement.setAttribute('data-mode', mode)
}
function applyFont(px: number) {
  document.documentElement.style.fontSize = `${px}px`
}

interface ThemeState {
  theme: Theme
  mode: Mode
  fontScale: number
  setTheme: (t: Theme) => void
  setMode: (m: Mode) => void
  toggleMode: () => void
  setFontScale: (px: number) => void
  bumpFont: (delta: number) => void
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: readTheme(),
  mode: readMode(),
  fontScale: readFont(),
  setTheme: (theme) => {
    applyTheme(theme)
    try { localStorage.setItem(THEME_KEY, theme) } catch { /* ignore */ }
    set({ theme })
  },
  setMode: (mode) => {
    applyMode(mode)
    try { localStorage.setItem(MODE_KEY, mode) } catch { /* ignore */ }
    set({ mode })
  },
  toggleMode: () => get().setMode(get().mode === 'dark' ? 'light' : 'dark'),
  setFontScale: (px) => {
    const clamped = Math.min(FONT_MAX, Math.max(FONT_MIN, px))
    applyFont(clamped)
    try { localStorage.setItem(FONT_KEY, String(clamped)) } catch { /* ignore */ }
    set({ fontScale: clamped })
  },
  bumpFont: (delta) => get().setFontScale(get().fontScale + delta),
}))

// Apply persisted values immediately (module load happens before React mount).
applyTheme(readTheme())
applyMode(readMode())
applyFont(readFont())
