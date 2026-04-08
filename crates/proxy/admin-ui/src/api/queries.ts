import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, mutatingFetch, mutatingFetchMultipart } from './client'
import { useAuthStore } from '../store/auth'
import type {
  Metrics, RequestsResponse, VirtualKey, KeySpend,
  Backend, ConfigResponse, ObservabilityResponse,
  ModelsResponse, AuditResponse, TrafficResponse, UptimeResponse,
  EnvImportResponse, ProxyStatus, DiscoverResponse,
  ManagedBackend, ManagedBackendsResponse, CreateManagedBackendRequest, UpdateManagedBackendRequest,
  CatalogProvider,
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
  })
}

export function useObservability(window: number, backend: string) {
  return useQuery<ObservabilityResponse>({
    queryKey: ['observability', window, backend],
    queryFn: () => apiFetch(`/admin/api/observability/overview?window=${window}&backend=${encodeURIComponent(backend)}`),
    refetchInterval: 30_000,
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
  query.set('page', String(params.page))
  query.set('page_size', String(params.page_size))
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
    queryFn: () => apiFetch('/admin/api/keys'),
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
    queryFn: () => apiFetch('/admin/api/backends'),
    staleTime: Infinity,
  })
}

// ── Settings / Config ─────────────────────────────────────────────────────────

export function useConfig() {
  return useQuery<ConfigResponse>({
    queryKey: ['config'],
    queryFn: () => apiFetch('/admin/api/config'),
    staleTime: Infinity,
  })
}

export function useSaveConfig() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: Record<string, string>) =>
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
    mutationFn: (body: Record<string, unknown>) =>
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
    queryFn: () => apiFetch(`/admin/api/audit?page=${params.page}&page_size=${params.page_size}`),
    staleTime: Infinity,
  })
}

// ── Traffic (new) ─────────────────────────────────────────────────────────────

export function useTraffic(windowHours: number) {
  return useQuery<TrafficResponse>({
    queryKey: ['traffic', windowHours],
    queryFn: () => apiFetch(`/admin/api/traffic?window=${windowHours}`),
    refetchInterval: 30_000,
  })
}

// ── Uptime (new) ──────────────────────────────────────────────────────────────

export function useUptime() {
  return useQuery<UptimeResponse>({
    queryKey: ['uptime'],
    queryFn: () => apiFetch('/admin/api/uptime'),
    refetchInterval: 30_000,
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
    queryFn: () => apiFetch('/admin/api/catalog/providers').then(r => r.providers ?? r),
    staleTime: Infinity,
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
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['managed-backends'] }) },
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
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['managed-backends'] }) },
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
