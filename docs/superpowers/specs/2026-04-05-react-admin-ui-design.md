# React Admin UI — Design Spec

**Date:** 2026-04-05  
**Status:** Approved  
**Scope:** Full rewrite of the admin UI from a monolithic vanilla JS SPA to a React + TypeScript application, shipped as a single release. Adds two new tabs: Traffic and Uptime.

---

## Context

The current admin UI is a 1,928-line single-file HTML/CSS/JS SPA embedded in the Rust binary via `include_str!`. It has no build tooling, no type safety, ~53 functions, and all state in loose `var` declarations at script scope. The goal is to replace it with a modular React + TypeScript application while preserving all existing backend contracts and the single-binary deployment model.

---

## Embedding Strategy

**Approach: single-bundle via `vite-plugin-singlefile`**

Vite builds the React app and `vite-plugin-singlefile` inlines all JS and CSS into a single `dist/index.html`. Rust continues using `include_str!("../admin-ui/dist/index.html")` with no changes to `serve_spa()`, the CSP nonce injection, or the security headers.

**Critical Vite config constraint:** `vite-plugin-singlefile` must be configured with `inlineScripts: false`. This preserves the `<script src="...">` tag structure rather than emitting a bare inline script, so the `__CSP_NONCE__` placeholder survives into the output for Rust to replace at request time. Without this, the nonce injection breaks.

`dist/` is gitignored. The built file is produced by the Docker frontend stage and copied into the Rust build stage.

---

## Build Pipeline

Four-stage Dockerfile:

```
Stage 1 — Frontend (node:20-alpine)
  npm ci
  vite build → admin-ui/dist/index.html

Stage 2 — Chef Prepare (rust:alpine + cargo-chef)
  cargo chef prepare → recipe.json

Stage 3 — Rust Build (rust:alpine)
  cargo chef cook --release
  COPY --from=stage1 admin-ui/dist/ crates/proxy/admin-ui/dist/
  cargo build --release -p anyllm_proxy

Stage 4 — Runtime (alpine:3)
  COPY binary only → ~25 MB image
```

CI (`ci.yml`) gains a frontend step before the Rust build: `npm ci && npx tsc --noEmit && npm run lint`. The Docker workflow is unchanged structurally — the new Node stage is prepended.

---

## Source Layout

```
crates/proxy/admin-ui/
  src/
    main.tsx                  # entry: QueryClientProvider, auth gate, App
    App.tsx                   # tab router, WS provider
    api/
      client.ts               # typed apiFetch() + mutatingFetch() (fetches CSRF token, then POSTs)
      queries.ts              # all React Query hooks (useMetrics, useRequests, useKeys, …)
      websocket.ts            # WS connect/reconnect, pushes events into Zustand ws store
      types.ts                # API response types (mirrors existing JSON shapes)
    store/
      auth.ts                 # Zustand: token (sessionStorage 'admin_token'), login(), logout()
      ws.ts                   # Zustand: status, lastEvent, connect(), disconnect()
    components/
      layout/
        Nav.tsx               # top nav bar with brand, tabs, WS status dot
        TabPanel.tsx          # renders active tab, handles tab switching
      shared/
        Badge.tsx             # status badges (active/revoked/expired/override)
        BudgetBar.tsx         # 4px progress bar with warn/danger thresholds
        DataTable.tsx         # reusable <table> with typed columns + row click
        LineChart.tsx         # SVG line chart (port of current renderLineChart)
        StatusDot.tsx         # animated pulse dot for live indicators
        Pagination.tsx        # prev/next with page label
        EmptyState.tsx        # centered empty / loading / error states
      feed/
        LiveFeed.tsx          # scrolling real-time request feed (WS-driven)
        FeedRow.tsx           # single feed row, expandable
        FeedDetail.tsx        # expanded row detail (amber left border)
    tabs/
      dashboard/
        Dashboard.tsx
        StatRow.tsx           # horizontal stat bar (RPM, error rate, latency, total)
        ObservabilityPanel.tsx# window/backend controls + charts
      traffic/                # NEW
        TrafficView.tsx       # route load table + req/min chart + payload bar chart
        RouteTable.tsx        # per-route: req/min, error rate, avg payload, P95, total
      uptime/                 # NEW
        UptimeView.tsx        # proxy health strip + per-backend health table
        ProxyHealth.tsx       # uptime %, running duration, 30-day history bar
        BackendHealthRow.tsx  # per-backend: status dot, uptime %, last check, history bar
      requests/
        RequestLog.tsx        # filterable paginated log (port of current tab)
      settings/
        Settings.tsx          # mutable settings form + env display + export
      keys/
        Keys.tsx
        KeyCreateForm.tsx
        KeyEditModal.tsx
      models/
        Models.tsx
      audit/
        Audit.tsx
  index.html                  # Vite entry HTML — has __CSP_NONCE__ on the <script> tag only; <style> is inlined by singlefile so its nonce is not needed
  vite.config.ts
  tsconfig.json
  package.json
  dist/                       # gitignored — built output
```

---

## State Management

| Layer | Tool | What it owns |
|---|---|---|
| Global / cross-tab | Zustand | Auth token, WS connection status + last event |
| Server state | React Query | All API responses, caching, polling intervals, loading/error states |
| Component-local | useState / useReducer | Form inputs, UI toggles, pagination cursor, expanded rows |

**No Redux. No Context for data.** Context is used only where React Query and Zustand are insufficient (none identified yet).

**React Query polling intervals:**

| Endpoint | Interval |
|---|---|
| `/admin/api/metrics` | 5s (dashboard live stats) |
| `/admin/api/observability/overview` | 30s |
| `/admin/api/traffic` | 30s |
| `/admin/api/uptime` | 30s |
| All other endpoints | `staleTime: Infinity`, refetch on tab focus or explicit user action |

---

## Backend Contracts (Unchanged)

The React frontend is a drop-in replacement. No existing backend contract changes.

| Contract | Detail |
|---|---|
| Auth header | `Authorization: Bearer <token>`, token in `sessionStorage['admin_token']` |
| CSRF | `GET /admin/csrf-token` before every mutating request; result in `X-CSRF-Token` header |
| WebSocket auth | Connect to `/admin/ws`, send `{"token":"..."}` as first message |
| Origin check | All requests are same-origin relative paths — no cross-origin calls |
| Admin API paths | All `/admin/api/*` paths unchanged |

`api/client.ts` exports:
- `apiFetch<T>(path, options?)` — adds `Authorization` header, returns typed `T`
- `mutatingFetch<T>(method, path, body?)` — fetches CSRF token first, then sends request with both `Authorization` and `X-CSRF-Token` headers

---

## New Backend Work (Rust)

Two new admin API endpoints and one background task. All other Rust code is unchanged.

### `GET /admin/api/traffic?window=6`

Aggregates the existing `request_log` SQLite table by route path and time bucket. No new data collection.

Response shape:
```json
{
  "window_hours": 6,
  "routes": [
    {
      "path": "POST /v1/messages",
      "requests_per_min": 4.2,
      "error_rate": 0.003,
      "avg_request_bytes": 2400,
      "p95_latency_ms": 1800,
      "total_requests": 1204
    }
  ],
  "series": [
    { "bucket_start": 1234567890, "path": "POST /v1/messages", "requests": 12 }
  ]
}
```

### `GET /admin/api/uptime`

Returns proxy start time and per-backend health check history (last 30 days, one record per day per backend aggregated from `health_checks` table).

Response shape:
```json
{
  "proxy": {
    "started_at": 1234567890,
    "uptime_pct_30d": 99.98,
    "history": [{ "date": "2026-04-05", "status": "up" }]
  },
  "backends": [
    {
      "name": "openai",
      "status": "up",
      "last_checked_at": 1234567890,
      "last_latency_ms": 210,
      "uptime_pct_30d": 99.9,
      "history": [{ "date": "2026-04-05", "status": "up" }]
    }
  ]
}
```

### SQLite migration: `health_checks` table

```sql
CREATE TABLE IF NOT EXISTS health_checks (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  backend     TEXT    NOT NULL,
  checked_at  INTEGER NOT NULL,  -- Unix timestamp
  status      TEXT    NOT NULL,  -- 'up' | 'down'
  latency_ms  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_health_checks_backend_time
  ON health_checks (backend, checked_at DESC);
```

Records older than 31 days are pruned on each write cycle.

### Background task: health checker

A Tokio task spawned at admin server startup. Every 30 seconds it probes each configured backend with a cheap HTTP request (GET `/v1/models` with a short 5s timeout). Results are written to `health_checks`. When a backend's status flips (up→down or down→up), it broadcasts a `backend_health_changed` WebSocket event to all connected admin clients so the Uptime tab updates immediately without waiting for the next poll.

New WS event shape:
```json
{ "type": "backend_health_changed", "backend": "gemini", "status": "down", "latency_ms": null }
```

---

## New Tab: Traffic

**Route:** shown when "Traffic" nav item is active.

**Data source:** `useTraffic(window)` React Query hook polling `GET /admin/api/traffic?window=N` every 30s. Window selector (1h / 6h / 24h) in tab header.

**Layout:**
1. Window selector (top right)
2. Route summary table: path | req/min | error rate | avg payload | P95 | total requests. Sorted by req/min descending.
3. Two charts side-by-side:
   - Requests/min over time, stacked by route (line chart, amber palette)
   - Avg payload size per route (bar chart, teal palette)

---

## New Tab: Uptime

**Route:** shown when "Uptime" nav item is active.

**Data source:** `useUptime()` React Query hook polling `GET /admin/api/uptime` every 30s. WS `backend_health_changed` events call `queryClient.invalidateQueries(['uptime'])` immediately on status flip.

**Layout:**
1. Proxy health strip: uptime %, running duration ("4d 3h 21m"), 30-day day bar (30 bars, each green or red). Full width.
2. "Backend Availability" section label.
3. Per-backend table: name | status dot | uptime % | last checked | 30-day history bar. Sorted by name.

History bars: 30 bars of equal width, each representing one calendar day. Green = all checks up that day. Red = any check failed. Amber = degraded (>0 failures but <50%). No bar = no data.

---

## Dependency List

```json
{
  "dependencies": {
    "react": "^19",
    "react-dom": "^19",
    "@tanstack/react-query": "^5",
    "zustand": "^5"
  },
  "devDependencies": {
    "typescript": "^5",
    "vite": "^6",
    "vite-plugin-singlefile": "^2",
    "@vitejs/plugin-react": "^4",
    "@types/react": "^19",
    "@types/react-dom": "^19"
  }
}
```

No UI component library. The existing CSS variable system (`--bg-base`, `--accent`, `--font-mono`, etc.) is preserved as a global stylesheet (`src/styles/globals.css`) imported once in `main.tsx`. CSS modules are not used — all class names match the existing system so the visual output is identical.

---

## CI Changes

`ci.yml` gains a frontend job that runs before the Rust build:

```yaml
- name: Frontend checks
  working-directory: crates/proxy/admin-ui
  run: |
    npm ci
    npx tsc --noEmit
    npm run lint
    npm run build
```

The Docker workflow's Dockerfile gains the Node stage (Stage 1) as described above. No other CI changes.

---

## Out of Scope

- Dark/light theme toggle
- i18n / localization
- Unit tests for React components (test infrastructure can be added in a follow-up)
- Storybook or component playground
- PWA / offline support
- Code splitting (deferred — adopt `include_dir` multi-file approach in a follow-up)
