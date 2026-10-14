import type {
  DashboardSnapshot_Serialize as DashboardSnapshot,
  DraftDetail,
  TagAssignmentRequest,
  TagAssignmentResponse,
  AvailabilityUpdate_Deserialize as AvailabilityUpdate,
} from './generated/bindings'

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message)
  }
}

// The wire shapes are generated from the Rust handlers' response models.
export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      Accept: 'application/json',
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...init?.headers,
    },
  })
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null)
    const message =
      typeof body === 'object' && body !== null && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `Request failed with HTTP ${response.status}`
    throw new ApiError(message, response.status)
  }
  return (await response.json()) as T
}
export const dashboardKey = ['dashboard'] as const
export const historyKey = (id: string) => ['history', id] as const
export function fetchDashboard({ signal }: { signal: AbortSignal }) {
  return request<DashboardSnapshot>('/api/dashboard/snapshot', { signal })
}
export async function fetchHistory(id: string, signal?: AbortSignal) {
  const body = await request<{ draft: DraftDetail }>(`/api/drafts/${encodeURIComponent(id)}`, {
    signal,
  })
  return body.draft
}
export function assignTags(id: string, names: string[]) {
  return request<TagAssignmentResponse>(`/api/dashboard/drafts/${encodeURIComponent(id)}/tags`, {
    method: 'POST',
    body: JSON.stringify({ names } satisfies TagAssignmentRequest),
  })
}
export function removeTag(id: string, tag: string) {
  return request(
    `/api/dashboard/drafts/${encodeURIComponent(id)}/tags/${encodeURIComponent(tag)}`,
    { method: 'DELETE' },
  )
}
export function updateAvailability(id: string, update: AvailabilityUpdate) {
  return request(`/api/drafts/${encodeURIComponent(id)}/availability`, {
    method: 'PUT',
    body: JSON.stringify(update),
  })
}
export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'The request failed.'
}
