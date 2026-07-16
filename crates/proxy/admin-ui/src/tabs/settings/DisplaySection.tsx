import { useThemeStore, THEMES, FONT_MIN, FONT_MAX } from '../../store/theme'

/**
 * Display section — accent color, light/dark mode (layered on top of the
 * accent), and text size. All client-side only (localStorage), no backend.
 */
export default function DisplaySection() {
  const theme = useThemeStore((s) => s.theme)
  const setTheme = useThemeStore((s) => s.setTheme)
  const mode = useThemeStore((s) => s.mode)
  const setMode = useThemeStore((s) => s.setMode)
  const fontScale = useThemeStore((s) => s.fontScale)
  const bumpFont = useThemeStore((s) => s.bumpFont)

  return (
    <div style={{ marginBottom: 24 }}>
      <div className="section-label" style={{ marginBottom: 8 }}>Display</div>

      <div className="display-controls">
        <div className="display-control">
          <div className="display-control-label">Accent color</div>
          <div className="theme-swatches">
            {THEMES.map((t) => (
              <button
                key={t.key}
                type="button"
                title={t.label}
                aria-label={`${t.label} accent`}
                aria-pressed={theme === t.key}
                className={`theme-swatch${theme === t.key ? ' active' : ''}`}
                style={{ background: t.swatch }}
                onClick={() => setTheme(t.key)}
              />
            ))}
          </div>
        </div>

        <div className="display-control">
          <div className="display-control-label">Mode</div>
          <div className="mode-toggle">
            {(['dark', 'light'] as const).map((m) => (
              <button
                key={m}
                type="button"
                aria-pressed={mode === m}
                className={`mode-toggle-btn${mode === m ? ' active' : ''}`}
                onClick={() => setMode(m)}
              >
                {m === 'dark' ? 'Dark' : 'Light'}
              </button>
            ))}
          </div>
        </div>

        <div className="display-control">
          <div className="display-control-label">Text size</div>
          <div className="font-scale">
            <button type="button" aria-label="Decrease text size" disabled={fontScale <= FONT_MIN} onClick={() => bumpFont(-1)}>A−</button>
            <span className="font-scale-value">{fontScale}px</span>
            <button type="button" aria-label="Increase text size" disabled={fontScale >= FONT_MAX} onClick={() => bumpFont(1)}>A+</button>
          </div>
        </div>
      </div>
    </div>
  )
}
