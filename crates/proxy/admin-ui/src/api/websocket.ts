import { useAuthStore } from '../store/auth'
import { useWsStore } from '../store/ws'
import type { WSEvent } from './types'

const MAX_RETRY_DELAY_MS = 30_000
const BASE_DELAY_MS = 1_000

let ws: WebSocket | null = null
let retryTimeout: ReturnType<typeof setTimeout> | null = null
let retryCount = 0
let stopped = false

export function connectWs(): void {
  stopped = false
  attemptConnect()
}

export function disconnectWs(): void {
  stopped = true
  if (retryTimeout) clearTimeout(retryTimeout)
  ws?.close()
  ws = null
  useWsStore.getState().setStatus('disconnected')
}

function attemptConnect(): void {
  if (stopped) return
  const token = useAuthStore.getState().token
  if (!token) return

  useWsStore.getState().setStatus('connecting')

  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
  ws = new WebSocket(`${protocol}//${location.host}/admin/ws`)

  ws.onopen = () => {
    ws!.send(JSON.stringify({ token }))
  }

  ws.onmessage = (evt) => {
    let data: unknown
    try { data = JSON.parse(evt.data) } catch { return }

    if (
      data !== null &&
      typeof data === 'object' &&
      'status' in data &&
      (data as { status: unknown }).status === 'authenticated'
    ) {
      retryCount = 0
      useWsStore.getState().setStatus('connected')
      return
    }

    if (
      data !== null &&
      typeof data === 'object' &&
      'type' in data
    ) {
      useWsStore.getState().pushEvent(data as WSEvent)
    }
  }

  ws.onclose = () => {
    if (stopped) return
    useWsStore.getState().setStatus('disconnected')
    const delay = Math.min(BASE_DELAY_MS * 2 ** retryCount, MAX_RETRY_DELAY_MS)
    retryCount++
    retryTimeout = setTimeout(attemptConnect, delay)
  }

  ws.onerror = () => {
    ws?.close()
  }
}
