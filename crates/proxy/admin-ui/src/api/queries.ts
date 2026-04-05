import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, mutatingFetch } from './client'
import type {
  Metrics, RequestsResponse, VirtualKey, KeySpend,
  Backend, ConfigResponse, ObservabilityResponse,
  ModelsResponse, AuditResponse, TrafficResponse, UptimeResponse,
} from './types'

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
      mutatingFetch<void>('POST', '/admin/api/config', body),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['config'] }) },
  })
}

export function useDeleteConfigOverride() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (key: string) =>
      mutatingFetch<void>('DELETE', `/admin/api/config/${encodeURIComponent(key)}`),
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
