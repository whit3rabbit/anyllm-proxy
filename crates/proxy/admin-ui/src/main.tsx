import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import App from './App'
import { RateLimitError } from './api/client'
import 'performative-ui/styles.css'
import './styles/globals.css'

// Default staleTime of 30s so tab switches within that window don't refetch
// everything from scratch. Live-data hooks (metrics, observability, traffic,
// uptime) override to 0 and rely on their own refetchInterval.
//
// `retry: 1` gives transient errors one free shot, but RateLimitError is
// explicitly not retried — retrying immediately just burns another bucket slot.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: (failureCount, error) => {
        if (error instanceof RateLimitError) return false
        return failureCount < 1
      },
      refetchOnWindowFocus: false,
      staleTime: 30_000,
    },
    mutations: {
      retry: (failureCount, error) => {
        if (error instanceof RateLimitError) return false
        return failureCount < 1
      },
    },
  },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
)
