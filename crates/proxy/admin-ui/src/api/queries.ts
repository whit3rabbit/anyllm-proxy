import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, mutatingFetch, mutatingFetchMultipart } from './client'
import { useAuthStore } from '../store/auth'
import type {
  Metrics, RequestsResponse, VirtualKey, KeySpend,
  Backend, ConfigEntry, ConfigResponse, ObservabilityResponse,
  ModelsResponse, AuditResponse, TrafficResponse, UptimeResponse,
  EnvImportResponse, ProxyStatus, DiscoverResponse,
  ManagedBackend, ManagedBackendsResponse, CreateManagedBackendRequest, UpdateManagedBackendRequest,
  CatalogProvider,
  Route, RoutesResponse, CreateRouteRequest, UpdateRouteRequest,
  RouteProvidersResponse, AddRouteProviderRequest, UpdateRouteProviderRequest,
  ReorderRouteProvidersRequest,
} from './types'

// ── Status ───────────────────────────────────────────────────────────────────

export function useStatus(enabled = true) {
  return useQuery<ProxyStatus>({
    queryKey: ['status'],
    queryFn: () => apiFetch('/admin/api/status'),
    enabled,
    staleTime: Infinity,
  })
}

// ── Dashboard ────────────────────────────────────────────────────────────────

export function useMetrics() {
  return useQuery<Metrics>({
    queryKey: ['metrics'],
    queryFn: () => apiFetch('/admin/api/metrics'),
    refetchInterval: 5_000,
    staleTime: 0,
  })
}

export function useObservability(window: number, backend: string) {
  return useQuery<ObservabilityResponse>({
    queryKey: ['observability', window, backend],
    queryFn: () => apiFetch(`/admin/api/observability/overview?window=${window}&backend=${encodeURIComponent(backend)}`),
    refetchInterval: 30_000,
    staleTime: 0,
  })
}

// ── Request log ───────────────────────────────────────────────────────────────

export function useRequests(params: {
  page: number
  page_size: number
  backend?: string
  status?: string
  since?: string
  until?: string
  model?: string
}) {
  const query = new URLSearchParams()
  // Backend paginates by limit/offset, not page/page_size. Keep the page-based
  // hook signature and translate at the boundary.
  query.set('limit', String(params.page_size))
  query.set('offset', String((params.page - 1) * params.page_size))
  if (params.backend) query.set('backend', params.backend)
  if (params.status) query.set('status', params.status)
  if (params.since) query.set('since', params.since)
  if (params.until) query.set('until', params.until)
  if (params.model) query.set('model', params.model)
  return useQuery<RequestsResponse>({
    queryKey: ['requests', params],
    queryFn: () => apiFetch(`/admin/api/requests?${query}`),
    staleTime: Infinity,
  })
}

// ── Virtual keys ──────────────────────────────────────────────────────────────

export function useKeys() {
  return useQuery<VirtualKey[]>({
    queryKey: ['keys'],
    // Backend returns { keys: [...] }; unwrap to the bare array consumers expect (mirrors useBackends).
    queryFn: () => apiFetch<{ keys: VirtualKey[] }>('/admin/api/keys').then(r => r.keys),
    staleTime: Infinity,
  })
}

export function useKeySpend(id: number) {
  return useQuery<KeySpend>({
    queryKey: ['keys', id, 'spend'],
    queryFn: () => apiFetch(`/admin/api/keys/${id}/spend`),
    staleTime: Infinity,
  })
}

export function useCreateKey() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      mutatingFetch<{ key: string; id: number }>('POST', '/admin/api/keys', body),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['keys'] }) },
  })
}

export function useUpdateKey() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: number; body: Record<string, unknown> }) =>
      mutatingFetch<VirtualKey>('PUT', `/admin/api/keys/${id}`, body),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['keys'] }) },
  })
}

export function useRevokeKey() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) =>
      mutatingFetch<void>('DELETE', `/admin/api/keys/${id}`),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['keys'] }) },
  })
}

// ── Backends ──────────────────────────────────────────────────────────────────

export function useBackends() {
  return useQuery<Backend[]>({
    queryKey: ['backends'],
    queryFn: () => apiFetch<{ backends: Backend[] }>('/admin/api/backends').then(r => r.backends),
    staleTime: Infinity,
  })
}

// ── Settings / Config ─────────────────────────────────────────────────────────

export function useConfig() {
  return useQuery<ConfigResponse>({
    queryKey: ['config'],
    queryFn: () =>
      Promise.all([
        apiFetch<Omit<ConfigResponse, 'entries' | 'env'>>('/admin/api/config'),
        apiFetch<{ overrides: ConfigEntry[] }>('/admin/api/config/overrides'),
      ]).then(([effective, overrides]) => ({
        ...effective,
        entries: overrides.overrides ?? [],
        env: {},
      })),
    staleTime: Infinity,
  })
}

export function useSaveConfig() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      mutatingFetch<void>('PUT', '/admin/api/config', body),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['config'] }) },
  })
}

export function useDeleteConfigOverride() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (key: string) =>
      mutatingFetch<void>('DELETE', `/admin/api/config/overrides/${encodeURIComponent(key)}`),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['config'] }) },
  })
}

export function useEnv() {
  return useQuery<Record<string, string>>({
    queryKey: ['env'],
    queryFn: () => apiFetch('/admin/api/env'),
    staleTime: Infinity,
  })
}

// ── Models ────────────────────────────────────────────────────────────────────

export function useModels() {
  return useQuery<ModelsResponse>({
    queryKey: ['models'],
    queryFn: () => apiFetch('/admin/api/models'),
    staleTime: Infinity,
  })
}

export function useAddModel() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: { model_name: string; actual_model: string; backend_name: string }) =>
      mutatingFetch<void>('POST', '/admin/api/models', body),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['models'] }) },
  })
}

export function useRemoveModel() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      mutatingFetch<void>('DELETE', `/admin/api/models/${encodeURIComponent(name)}`),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['models'] }) },
  })
}

export function useDiscoverModels() {
  return useMutation<DiscoverResponse, Error, { source: string; url?: string }>({
    mutationFn: (body) =>
      mutatingFetch<DiscoverResponse>('POST', '/admin/api/models/discover', body),
  })
}

// ── Audit ─────────────────────────────────────────────────────────────────────

export function useAudit(params: { page: number; page_size: number }) {
  return useQuery<AuditResponse>({
    queryKey: ['audit', params],
    // Backend paginates by limit/offset, not page/page_size; translate here.
    queryFn: () => apiFetch(`/admin/api/audit?limit=${params.page_size}&offset=${(params.page - 1) * params.page_size}`),
    staleTime: Infinity,
  })
}

// ── Traffic (new) ─────────────────────────────────────────────────────────────

export function useTraffic(windowHours: number) {
  return useQuery<TrafficResponse>({
    queryKey: ['traffic', windowHours],
    queryFn: () => apiFetch(`/admin/api/traffic?window=${windowHours}`),
    refetchInterval: 30_000,
    staleTime: 0,
  })
}

// ── Uptime (new) ──────────────────────────────────────────────────────────────

export function useUptime() {
  return useQuery<UptimeResponse>({
    queryKey: ['uptime'],
    queryFn: () => apiFetch('/admin/api/uptime'),
    refetchInterval: 30_000,
    staleTime: 0,
  })
}

// ── Env file import / export ──────────────────────────────────────────────────

/** Upload a .anyllm.env file to the proxy. Returns parse warnings on success. */
export function useImportEnv() {
  return useMutation<EnvImportResponse, Error, File>({
    mutationFn: (file: File) => {
      const fd = new FormData()
      fd.append('file', file)
      return mutatingFetchMultipart<EnvImportResponse>('/admin/api/env/import', fd)
    },
  })
}

// ── Catalog / Provider metadata ────────────────────────────────────────────────

export function useCatalogProviders() {
  return useQuery<CatalogProvider[]>({
    queryKey: ['catalog-providers'],
    queryFn: () => apiFetch<{ providers: CatalogProvider[] }>('/admin/api/catalog/providers').then(r => r.providers),
    staleTime: Infinity,
  })
}

export function useRefreshProvider() {
  const qc = useQueryClient()
  return useMutation<{ provider_id: string; count: number; models: string[] }, Error, string>({
    mutationFn: (providerId: string) =>
      mutatingFetch('POST', `/admin/api/catalog/providers/${encodeURIComponent(providerId)}/refresh`, {}),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['catalog-providers'] })
    },
  })
}

export function useCatalogProviderModels(providerId: string | null) {
  return useQuery<{ provider_id: string; has_models: boolean; models: object[]; cached_models: string[] }>({
    queryKey: ['catalog-provider-models', providerId],
    queryFn: () => apiFetch(`/admin/api/catalog/providers/${encodeURIComponent(providerId!)}/models`),
    enabled: !!providerId,
    staleTime: 30_000,
  })
}

// ── Managed backends ──────────────────────────────────────────────────────────

export function useManagedBackends() {
  return useQuery<ManagedBackendsResponse>({
    queryKey: ['managed-backends'],
    queryFn: () => apiFetch('/admin/api/backends/managed'),
    staleTime: Infinity,
  })
}

export function useCreateManagedBackend() {
  const qc = useQueryClient()
  return useMutation<ManagedBackend, Error, CreateManagedBackendRequest>({
    mutationFn: (data) =>
      mutatingFetch<ManagedBackend>('POST', '/admin/api/backends/managed', data),
    // Adding the first managed backend flips /admin/api/status configured -> true.
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['managed-backends'] })
      qc.invalidateQueries({ queryKey: ['status'] })
    },
  })
}

export function useUpdateManagedBackend() {
  const qc = useQueryClient()
  return useMutation<ManagedBackend, Error, { name: string; data: UpdateManagedBackendRequest }>({
    mutationFn: ({ name, data }) =>
      mutatingFetch<ManagedBackend>('PUT', `/admin/api/backends/managed/${name}`, data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['managed-backends'] }) },
  })
}

export function useDeleteManagedBackend() {
  const qc = useQueryClient()
  return useMutation<void, Error, string>({
    mutationFn: (name) =>
      mutatingFetch<void>('DELETE', `/admin/api/backends/managed/${name}`),
    // Deleting the last managed backend can flip configured -> false.
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['managed-backends'] })
      qc.invalidateQueries({ queryKey: ['status'] })
    },
  })
}

// ── Routes ────────────────────────────────────────────────────────────────────

export function useRoutes() {
  return useQuery<RoutesResponse>({
    queryKey: ['routes'],
    queryFn: () => apiFetch('/admin/api/routes'),
    staleTime: Infinity,
  })
}

export function useCreateRoute() {
  const qc = useQueryClient()
  return useMutation<Route, Error, CreateRouteRequest>({
    mutationFn: (data) => mutatingFetch<Route>('POST', '/admin/api/routes', data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['routes'] }) },
  })
}

export function useUpdateRoute() {
  const qc = useQueryClient()
  return useMutation<Route, Error, { id: string; data: UpdateRouteRequest }>({
    mutationFn: ({ id, data }) => mutatingFetch<Route>('PUT', `/admin/api/routes/${id}`, data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['routes'] }) },
  })
}

export function useDeleteRoute() {
  const qc = useQueryClient()
  return useMutation<void, Error, string>({
    mutationFn: (id) => mutatingFetch<void>('DELETE', `/admin/api/routes/${id}`),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['routes'] }) },
  })
}

export function useRouteProviders(routeId: string | null) {
  return useQuery<RouteProvidersResponse>({
    queryKey: ['route-providers', routeId],
    queryFn: () => apiFetch(`/admin/api/routes/${routeId}/providers`),
    enabled: !!routeId,
    staleTime: Infinity,
  })
}

export function useAddRouteProvider() {
  const qc = useQueryClient()
  return useMutation<void, Error, { routeId: string; data: AddRouteProviderRequest }>({
    mutationFn: ({ routeId, data }) =>
      mutatingFetch<void>('POST', `/admin/api/routes/${routeId}/providers`, data),
    onSuccess: (_d, { routeId }) => {
      qc.invalidateQueries({ queryKey: ['route-providers', routeId] })
      qc.invalidateQueries({ queryKey: ['routes'] })
    },
  })
}

export function useUpdateRouteProvider() {
  const qc = useQueryClient()
  return useMutation<void, Error, { routeId: string; providerId: string; data: UpdateRouteProviderRequest }>({
    mutationFn: ({ routeId, providerId, data }) =>
      mutatingFetch<void>('PUT', `/admin/api/routes/${routeId}/providers/${providerId}`, data),
    onSuccess: (_d, { routeId }) => {
      qc.invalidateQueries({ queryKey: ['route-providers', routeId] })
    },
  })
}

export function useRemoveRouteProvider() {
  const qc = useQueryClient()
  return useMutation<void, Error, { routeId: string; providerId: string }>({
    mutationFn: ({ routeId, providerId }) =>
      mutatingFetch<void>('DELETE', `/admin/api/routes/${routeId}/providers/${providerId}`),
    onSuccess: (_d, { routeId }) => {
      qc.invalidateQueries({ queryKey: ['route-providers', routeId] })
      qc.invalidateQueries({ queryKey: ['routes'] })
    },
  })
}

export function useReorderRouteProviders() {
  const qc = useQueryClient()
  return useMutation<RouteProvidersResponse, Error, { routeId: string; data: ReorderRouteProvidersRequest }>({
    mutationFn: ({ routeId, data }) =>
      mutatingFetch<RouteProvidersResponse>('PUT', `/admin/api/routes/${routeId}/providers/reorder`, data),
    onSuccess: (_d, { routeId }) => {
      qc.invalidateQueries({ queryKey: ['route-providers', routeId] })
      qc.invalidateQueries({ queryKey: ['routes'] })
    },
  })
}

/**
 * Download the current effective env as a .anyllm.env file.
 * Not a hook — call directly from an event handler.
 */
export async function downloadEnvExport(): Promise<void> {
  const token = useAuthStore.getState().token ?? ''
  const res = await fetch('/admin/api/env/export', {
    headers: { 'Authorization': `Bearer ${token}` },
  })
  if (!res.ok) throw new Error(`Export failed: HTTP ${res.status}`)
  const blob = await res.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = '.anyllm.env'
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}
