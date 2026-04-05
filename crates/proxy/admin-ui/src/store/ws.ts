import { create } from 'zustand'
import type { WSEvent } from '../api/types'

type WsStatus = 'disconnected' | 'connecting' | 'connected'

interface WsState {
  status: WsStatus
  lastEvent: WSEvent | null
  setStatus: (status: WsStatus) => void
  pushEvent: (event: WSEvent) => void
}

export const useWsStore = create<WsState>((set) => ({
  status: 'disconnected',
  lastEvent: null,
  setStatus: (status) => set({ status }),
  pushEvent: (event) => set({ lastEvent: event }),
}))
