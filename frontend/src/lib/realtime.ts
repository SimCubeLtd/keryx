import type { QueryClient } from '@tanstack/svelte-query'
import { dashboardKey } from './api'

// Serialize invalidations and retain a trailing refresh when a change arrives mid-request.
export function dashboardRefresh(client: QueryClient) {
  let running = false
  let pending = false
  let closed = false
  async function refresh() {
    if (closed) return
    if (running) {
      pending = true
      return
    }
    running = true
    try {
      do {
        const requestAlreadyRunning =
          client.isFetching({ queryKey: dashboardKey }) > 0 ||
          client.isFetching({ queryKey: ['history'] }) > 0
        pending = false
        await Promise.all([
          client.invalidateQueries({ queryKey: dashboardKey }, { cancelRefetch: false }),
          client.invalidateQueries({ queryKey: ['history'] }, { cancelRefetch: false }),
        ])
        pending ||= requestAlreadyRunning
      } while (pending && !closed)
    } finally {
      running = false
    }
  }
  return {
    refresh,
    close() {
      closed = true
    },
  }
}
export function connectDashboard(refresh: () => Promise<void>) {
  if (!('EventSource' in window)) return () => {}
  const events = new EventSource('/api/dashboard/events')
  const listener = () => {
    void refresh()
  }
  events.addEventListener('dashboard', listener)
  return () => {
    events.close?.()
  }
}
