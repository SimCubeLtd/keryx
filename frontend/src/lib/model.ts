import type { Availability, DashboardDraft, Tag } from './generated/bindings'
export type { Availability } from './generated/bindings'
export type Row = DashboardDraft['summary'] & { tags: Tag[]; availability: Availability }
export type Sort = 'updated' | 'oldest' | 'title' | 'versions' | 'tag'
export function availability(draft: DashboardDraft['summary'], now: number): Availability {
  return draft.disabled
    ? 'disabled'
    : Date.parse(draft.snoozedUntil ?? '') > now
      ? 'snoozed'
      : 'active'
}
export function repository(row: Row) {
  return row.repoOrg && row.repoName ? `${row.repoOrg}/${row.repoName}` : ''
}
export function matches(row: Row, view: Availability, search: string, repo: string) {
  const text = [
    row.title,
    row.description,
    row.draftId,
    repository(row),
    row.latestGitBranch,
    ...row.tags.map((tag) => tag.name),
  ]
    .join(' ')
    .toLocaleLowerCase()
  return (
    row.availability === view &&
    (!repo || repository(row) === repo) &&
    text.includes(search.trim().toLocaleLowerCase())
  )
}
export function compareRows(sort: Sort) {
  return (a: Row, b: Row) => {
    if (sort === 'title') return a.title.localeCompare(b.title)
    if (sort === 'versions') return (b.versionCount ?? 0) - (a.versionCount ?? 0)
    if (sort === 'tag') {
      const left = a.tags[0]?.name,
        right = b.tags[0]?.name
      return (
        Number(!left) - Number(!right) ||
        (left && right ? left.localeCompare(right) : 0) ||
        a.title.localeCompare(b.title) ||
        a.draftId.localeCompare(b.draftId)
      )
    }
    const delta = Date.parse(b.updatedAt) - Date.parse(a.updatedAt)
    return sort === 'oldest' ? -delta : delta
  }
}
export function canonicalTag(value: string) {
  if (!/^[a-zA-Z0-9 -]*$/.test(value)) return null
  const name = value.trim().replace(/ +/g, ' ').toLowerCase()
  return name.length && name.length <= 32 ? name : null
}
export function relativeTime(value: string, now = Date.now()) {
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return value || 'Not recorded'
  const minutes = Math.floor(Math.max(0, now - time) / 60000)
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes}m ago`
  if (minutes < 1440) return `${Math.floor(minutes / 60)}h ago`
  if (minutes < 20160) return `${Math.floor(minutes / 1440)}d ago`
  return new Date(time).toLocaleDateString()
}
export function wakeLabel(value: string | null | undefined) {
  if (!value || !Number.isFinite(Date.parse(value))) return ''
  return new Date(value).toLocaleString(undefined, {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  })
}
export function relativeFuture(value: string | null | undefined, now = Date.now()) {
  const minutes = Math.floor(Math.max(0, Date.parse(value ?? '') - now) / 60000)
  if (minutes < 1) return 'in under a minute'
  const count =
    minutes < 60 ? minutes : minutes < 2880 ? Math.floor(minutes / 60) : Math.floor(minutes / 1440)
  const unit = minutes < 60 ? 'minute' : minutes < 2880 ? 'hour' : 'day'
  return `in ${count} ${unit}${count === 1 ? '' : 's'}`
}
export function fileSize(bytes: number) {
  return bytes < 1024
    ? `${bytes} B`
    : bytes < 1048576
      ? `${Math.round(bytes / 1024)} KB`
      : `${(bytes / 1048576).toFixed(1)} MB`
}
export function canReset(row: Row) {
  return (row.versionCount ?? 0) > 1 || (row.latestVersionNumber ?? 0) > 1
}
