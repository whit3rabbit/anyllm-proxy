/**
 * Inline nav icons for the sidebar. Keyed by route path so the Sidebar can
 * look up the right glyph for each item. Stroke inherits `currentColor`, so
 * active/hover coloring is handled entirely by CSS.
 */

const P = {
  stroke: 'currentColor',
  fill: 'none',
  strokeWidth: 1.7,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
}

export const NAV_ICONS: Record<string, React.ReactNode> = {
  '/dashboard': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </svg>
  ),
  '/requests': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <line x1="8" y1="6" x2="21" y2="6" />
      <line x1="8" y1="12" x2="21" y2="12" />
      <line x1="8" y1="18" x2="21" y2="18" />
      <circle cx="3.5" cy="6" r="1" />
      <circle cx="3.5" cy="12" r="1" />
      <circle cx="3.5" cy="18" r="1" />
    </svg>
  ),
  '/traffic': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <polyline points="3 12 7 12 10 5 14 19 17 12 21 12" />
    </svg>
  ),
  '/providers': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <rect x="3" y="4" width="18" height="7" rx="2" />
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <line x1="7" y1="7.5" x2="7" y2="7.5" />
      <line x1="7" y1="16.5" x2="7" y2="16.5" />
    </svg>
  ),
  '/routing': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="6" cy="18" r="2.5" />
      <circle cx="18" cy="12" r="2.5" />
      <path d="M8.5 6H14a2 2 0 0 1 2 2v1.5M8.5 18H14a2 2 0 0 0 2-2v-1.5" />
    </svg>
  ),
  '/routes': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <circle cx="5" cy="19" r="2" />
      <circle cx="19" cy="5" r="2" />
      <path d="M5 17V9a4 4 0 0 1 4-4h6" />
      <polyline points="13 3 16 5 13 7" />
    </svg>
  ),
  '/models': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <path d="M12 2 21 7v10l-9 5-9-5V7z" />
      <path d="M3.5 7.5 12 12l8.5-4.5M12 12v9.5" />
    </svg>
  ),
  '/backends': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <ellipse cx="12" cy="5" rx="8" ry="3" />
      <path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5" />
      <path d="M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
    </svg>
  ),
  '/keys': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <circle cx="8" cy="8" r="4" />
      <path d="M11 11l8 8M16 16l2-2M19 19l2-2" />
    </svg>
  ),
  '/audit': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <path d="M12 3l7 3v5c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V6z" />
      <polyline points="9 12 11 14 15 10" />
    </svg>
  ),
  '/settings': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" />
    </svg>
  ),
  '/uptime': (
    <svg width="18" height="18" viewBox="0 0 24 24" {...P}>
      <path d="M3 12h4l2-6 4 12 2-6h6" />
    </svg>
  ),
}
