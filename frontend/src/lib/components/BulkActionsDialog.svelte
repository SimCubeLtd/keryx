<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte'
  import type {
    DraftDetail,
    PruneVersionsResponse,
    PurgeDraftResponse,
    PruneVersionsRequest,
  } from '$lib/generated/bindings'
  import { ApiError, errorMessage, fetchHistory, request } from '$lib/api'
  import { repository, type Row } from '$lib/model'
  import * as Dialog from './ui/dialog'
  import { Button } from './ui/button'
  import { Input } from './ui/input'

  let {
    targets,
    action,
    rows,
    refresh,
    onconfirm,
    onclose,
  }: {
    targets: Row[]
    action: 'reset' | 'purge'
    rows: Row[]
    refresh: () => Promise<void>
    onconfirm: () => void
    onclose: () => void
  } = $props()
  type Status =
    | 'ready'
    | 'running'
    | 'reset'
    | 'deleted'
    | 'unchanged'
    | 'failed'
    | 'conflict'
    | 'missing'
    | 'unknown'
    | 'unattempted'
  const statusLabels = {
    ready: 'Waiting',
    running: 'In progress',
    reset: 'History reset',
    deleted: 'Deleted',
    unchanged: 'Unchanged',
    failed: 'Failed',
    conflict: 'Changed since review',
    missing: 'Already unavailable',
    unknown: 'Outcome unknown',
    unattempted: 'Not attempted',
  } satisfies Record<Status, string>
  type Item = { target: Row; detail?: DraftDetail; status: Status; message?: string }
  let items = $state<Item[]>(untrack(() => targets.map((target) => ({ target, status: 'ready' }))))
  let loading = $state(true),
    running = $state(false),
    finished = $state(false),
    stopping = $state(false)
  let confirmation = $state(''),
    error = $state('')
  let alive = true
  const count = $derived(items.length)
  const versionCount = $derived(
    items.reduce((sum, item) => sum + (item.detail?.versions.length ?? 0), 0),
  )
  const olderCount = $derived(
    items.reduce((sum, item) => sum + Math.max(0, (item.detail?.versions.length ?? 0) - 1), 0),
  )
  const processed = $derived(
    items.filter((item) => !['ready', 'running', 'unattempted'].includes(item.status)).length,
  )
  const ready = $derived(items.every((item) => !!item.detail?.versions[0]))
  const stale = $derived(
    !loading &&
      !running &&
      !finished &&
      items.some((item) => {
        const current = rows.find((row) => row.draftId === item.target.draftId)
        const reviewed = item.detail?.draft
        return (
          !current ||
          !reviewed ||
          current.updatedAt !== reviewed.updatedAt ||
          current.versionCount !== reviewed.versionCount ||
          current.latestVersionNumber !== reviewed.latestVersionNumber ||
          current.disabled !== reviewed.disabled ||
          current.snoozedUntil !== reviewed.snoozedUntil
        )
      }),
  )
  const successful = $derived(
    items.filter((item) => ['reset', 'deleted', 'unchanged'].includes(item.status)).length,
  )
  const problems = $derived(
    items.filter((item) => ['failed', 'conflict', 'missing', 'unknown'].includes(item.status))
      .length,
  )

  onDestroy(() => {
    alive = false
    stopping = true
  })
  onMount(() => {
    void review()
  })
  async function review() {
    loading = true
    confirmation = ''
    error = ''
    for (let index = 0; index < targets.length; index++) {
      const target = targets[index]
      if (!target || !alive) return
      try {
        const detail = await fetchHistory(target.draftId)
        if (!alive) return
        items[index] = { target, detail, status: 'ready' }
      } catch (cause) {
        if (!alive) return
        items[index] = { target, status: 'failed', message: errorMessage(cause) }
      }
    }
    await refresh().catch(() => {
      if (alive) error = 'Could not refresh the dashboard. Try reviewing again.'
    })
    if (alive) loading = false
  }
  async function execute() {
    if (
      loading ||
      running ||
      finished ||
      !ready ||
      stale ||
      (action === 'purge' && confirmation !== 'DELETE')
    )
      return
    running = true
    onconfirm()
    for (let index = 0; index < items.length; index++) {
      const item = items[index]
      if (!item || !alive || stopping) break
      const latest = item.detail?.versions[0]
      if (!latest) continue
      if (action === 'reset' && item.detail?.versions.length === 1 && latest.versionNumber === 1) {
        item.status = 'unchanged'
        item.message = 'Already a single v1; nothing changed.'
        continue
      }
      item.status = 'running'
      try {
        const path = `/api/drafts/${encodeURIComponent(item.target.draftId)}`
        const result =
          action === 'reset'
            ? await request<PruneVersionsResponse>(`${path}/versions/prune`, {
                method: 'POST',
                body: JSON.stringify({
                  expectedCurrentVersionId: latest.id,
                } satisfies PruneVersionsRequest),
              })
            : await request<PurgeDraftResponse>(
                `${path}?purge=true&expectedCurrentVersionId=${encodeURIComponent(latest.id)}`,
                { method: 'DELETE' },
              )
        if (!alive) return
        item.status = action === 'reset' ? 'reset' : 'deleted'
        item.message = result.cleanupComplete
          ? action === 'reset'
            ? 'Latest content kept as v1.'
            : 'Draft and all versions deleted.'
          : 'Metadata committed; storage cleanup pending. Run keryx storage gc.'
      } catch (cause) {
        if (!alive) return
        if (!(cause instanceof ApiError)) {
          item.status = 'unknown'
          item.message = 'Outcome unknown. Refresh and inspect this draft before trying again.'
          stopping = true
        } else {
          item.status =
            cause.status === 409 ? 'conflict' : cause.status === 404 ? 'missing' : 'failed'
          item.message =
            cause.status === 404
              ? 'Already unavailable; this does not confirm deletion.'
              : errorMessage(cause)
          if (cause.status === 401 || cause.status === 403) stopping = true
        }
      }
    }
    if (!alive) return
    for (const item of items) if (item.status === 'ready') item.status = 'unattempted'
    running = false
    finished = true
    await refresh().catch(() => {
      if (alive) error = 'Results are recorded, but the dashboard could not refresh.'
    })
  }
</script>

<Dialog.Root
  open={true}
  onOpenChange={(open) => {
    if (!open && !running) onclose()
  }}
>
  <Dialog.Content
    id="bulk-dialog"
    class="bulk-dialog"
    escapeKeydownBehavior={running ? 'ignore' : 'close'}
    interactOutsideBehavior={running ? 'ignore' : 'close'}
  >
    <Dialog.Title
      >{finished
        ? 'Bulk action results'
        : action === 'reset'
          ? `Reset and prune ${count} ${count === 1 ? 'history' : 'histories'}?`
          : `Delete ${count} ${count === 1 ? 'draft' : 'drafts'} permanently?`}</Dialog.Title
    >
    <Dialog.Description>
      {#if finished}{successful} completed or unchanged · {problems} need attention · {items.filter(
          (item) => item.status === 'unattempted',
        ).length} not attempted.
      {:else if loading}Loading the selected drafts’ histories for review…
      {:else if action === 'reset'}Keep {count}
        {count === 1 ? 'draft' : 'drafts'} and their latest content. Remove {olderCount}
        older {olderCount === 1 ? 'version' : 'versions'}, reset each history to v1, and make the
        next upload v2.
      {:else}Delete {count}
        {count === 1 ? 'draft' : 'drafts'} and all {versionCount} stored {versionCount === 1
          ? 'version'
          : 'versions'}. Public and versioned links will stop working.{/if}
    </Dialog.Description>
    {#if running}<p class="bulk-progress" role="status">
        {processed} of {count} processed{stopping ? ' · stopping after the current draft' : ''}
      </p>{/if}
    <div class="bulk-targets" aria-label="Selected drafts">
      {#each items as item (item.target.draftId)}
        <div class="bulk-target" data-bulk-status={item.status}>
          <strong>{item.detail?.draft.title ?? item.target.title}</strong>
          <span>{repository(item.target) || 'No repository'} · {item.target.draftId}</span>
          <span
            >{item.detail
              ? `${item.detail.versions.length} ${item.detail.versions.length === 1 ? 'version' : 'versions'}${action === 'reset' ? ` · v${item.detail.versions[0]?.versionNumber ?? 0} → v1` : ''}`
              : 'History unavailable'}</span
          >
          {#if !running && !finished && action === 'reset' && item.detail?.versions.length === 1 && item.detail.versions[0]?.versionNumber === 1}<p
            >
              Already a single v1; will be skipped.
            </p>{/if}
          {#if running || finished}<b>{statusLabels[item.status]}</b>{/if}
          {#if item.message}<p>{item.message}</p>{/if}
        </div>
      {/each}
    </div>
    {#if !finished}
      <p class="dialog-warning">
        {action === 'reset'
          ? 'Older versions are permanently deleted. Versioned links may stop working or show different content. Main draft links, tags and availability stay unchanged.'
          : 'This cannot be undone. Every version of each selected draft will be permanently deleted.'}
      </p>
      {#if stale}<p class="dialog-error" role="alert">
          A selected draft changed or is no longer available. Review again before continuing.
        </p>{/if}
      {#if action === 'purge'}<label class="bulk-confirm-label" for="bulk-confirm-text"
          >Type DELETE to confirm</label
        ><Input
          id="bulk-confirm-text"
          bind:value={confirmation}
          disabled={loading || running}
          autocomplete="off"
          placeholder="Type DELETE…"
        />{/if}
    {/if}
    {#if error}<p class="dialog-error" role="alert">{error}</p>{/if}
    <div class="dialog-actions">
      {#if running}<Button
          variant="outline"
          disabled={stopping}
          onclick={() => {
            stopping = true
          }}>Stop after current draft</Button
        >
      {:else if finished}<Button id="bulk-close" onclick={onclose}>Close</Button>
      {:else}
        <Button variant="outline" onclick={onclose}>Cancel</Button>
        {#if !loading && (stale || !ready || error)}<Button
            variant="outline"
            onclick={() => {
              void review()
            }}>Review again</Button
          >{/if}
        <Button
          id="bulk-confirm"
          variant="destructive"
          disabled={loading ||
            !ready ||
            stale ||
            !!error ||
            (action === 'purge' && confirmation !== 'DELETE')}
          onclick={() => {
            void execute()
          }}
          >{action === 'reset'
            ? `Reset and prune ${count} ${count === 1 ? 'history' : 'histories'}`
            : `Delete ${count} ${count === 1 ? 'draft' : 'drafts'} permanently`}</Button
        >
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
