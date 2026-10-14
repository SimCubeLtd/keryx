import { QueryClient, QueryObserver } from '@tanstack/svelte-query'
import { expect, test, vi } from 'vite-plus/test'
import { dashboardKey } from './api'
import { dashboardRefresh } from './realtime'

function controlledDashboard(cached: boolean) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  if (cached) client.setQueryData(dashboardKey, 0)
  const releases: (() => void)[] = []
  const fetch = vi.fn(
    () =>
      new Promise<number>((resolve) => {
        releases.push(() => resolve(releases.length))
      }),
  )
  const observer = new QueryObserver(client, { queryKey: dashboardKey, queryFn: fetch })
  const unsubscribe = observer.subscribe(() => {})
  const updates = dashboardRefresh(client)
  return {
    fetch,
    releases,
    updates,
    cleanup() {
      updates.close()
      unsubscribe()
      client.clear()
    },
  }
}

test('a burst during a fetch produces one trailing refresh with fresh data', async () => {
  const dashboard = controlledDashboard(true)
  try {
    const completed = dashboard.updates.refresh()
    await dashboard.updates.refresh()
    await dashboard.updates.refresh()
    expect(dashboard.fetch).toHaveBeenCalledTimes(1)
    dashboard.releases[0]?.()
    await vi.waitFor(() => expect(dashboard.fetch).toHaveBeenCalledTimes(2))
    dashboard.releases[1]?.()
    await completed
    expect(dashboard.fetch).toHaveBeenCalledTimes(2)
  } finally {
    dashboard.cleanup()
  }
})

test('an event during the initial query also fetches again after that request', async () => {
  const dashboard = controlledDashboard(false)
  try {
    expect(dashboard.fetch).toHaveBeenCalledTimes(1)
    const completed = dashboard.updates.refresh()
    dashboard.releases[0]?.()
    await vi.waitFor(() => expect(dashboard.fetch).toHaveBeenCalledTimes(2))
    dashboard.releases[1]?.()
    await completed
  } finally {
    dashboard.cleanup()
  }
})

test('closing while a request is running suppresses its queued trailing refresh', async () => {
  const dashboard = controlledDashboard(true)
  try {
    const completed = dashboard.updates.refresh()
    await dashboard.updates.refresh()
    dashboard.updates.close()
    dashboard.releases[0]?.()
    await completed
    expect(dashboard.fetch).toHaveBeenCalledTimes(1)
  } finally {
    dashboard.cleanup()
  }
})
