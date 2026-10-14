<script lang="ts">
  import { onDestroy, onMount } from 'svelte'
  import type { DraftDetail, PruneVersionsResponse } from '$lib/generated/bindings'
  import type { Row } from '$lib/model'
  import { ApiError, errorMessage, fetchHistory, request } from '$lib/api'
  import * as Dialog from './ui/dialog'
  import { Button } from './ui/button'
  let {
    target,
    onclose,
    refresh,
    toast,
  }: {
    target: Row
    onclose: () => void
    refresh: () => Promise<void>
    toast: (message: string, error?: boolean) => void
  } = $props()
  let detail = $state<DraftDetail>(),
    error = $state(''),
    pending = $state(false)
  let alive = true
  onDestroy(() => {
    alive = false
  })
  const latest = $derived(detail?.versions[0])
  const removed = $derived(Math.max(0, (detail?.versions.length ?? 0) - 1))
  const nothing = $derived(!!latest && removed === 0 && latest.versionNumber === 1)
  async function load() {
    const response = await fetchHistory(target.draftId)
    if (alive) detail = response
  }
  onMount(() => {
    void load().catch((cause) => {
      if (alive) error = errorMessage(cause)
    })
  })
  async function submit() {
    if (!latest || pending || nothing) return
    pending = true
    error = ''
    try {
      const result = await request<PruneVersionsResponse>(
        `/api/drafts/${encodeURIComponent(target.draftId)}/versions/prune`,
        { method: 'POST', body: JSON.stringify({ expectedCurrentVersionId: latest.id }) },
      )
      if (alive) onclose()
      await refresh()
      toast(
        result.cleanupComplete
          ? 'Version history reset to v1. Older versions pruned.'
          : 'History reset to v1, but stored file cleanup is incomplete. Run keryx storage gc to remove orphan files.',
        !result.cleanupComplete,
      )
    } catch (cause) {
      if (!alive) return
      error = errorMessage(cause)
      if (cause instanceof ApiError && cause.status === 409) {
        try {
          await load()
          await refresh()
        } catch (loadError) {
          if (alive) error = errorMessage(loadError)
        }
      }
    } finally {
      if (alive) pending = false
    }
  }
</script>

<Dialog.Root
  open={true}
  onOpenChange={(open) => {
    if (!open && !pending) onclose()
  }}
>
  <Dialog.Content
    id="reset-history-dialog"
    escapeKeydownBehavior={pending ? 'ignore' : 'close'}
    interactOutsideBehavior={pending ? 'ignore' : 'close'}
  >
    <Dialog.Title id="reset-history-title">Reset and prune version history?</Dialog.Title>
    <Dialog.Description id="reset-history-description"
      >{!latest
        ? 'Loading version history…'
        : nothing
          ? 'This draft already contains only v1. There is nothing to reset or prune.'
          : `Permanently delete ${removed} older ${removed === 1 ? 'version' : 'versions'} and renumber v${latest.versionNumber} as v1. The next upload will be v2.`}</Dialog.Description
    >
    <div class="dialog-summary">
      <strong id="reset-history-draft-title">{detail?.draft.title ?? target.title}</strong><span
        id="reset-history-draft-id">{target.draftId}</span
      >
    </div>
    <div class="reset-history-facts">
      <div>
        <strong>Keep and renumber</strong><span id="reset-history-kept"
          >{latest ? `v${latest.versionNumber} → v1 · latest content` : 'Loading…'}</span
        >
      </div>
      <div>
        <strong>Delete permanently</strong><span id="reset-history-removed"
          >{latest
            ? `${removed} older ${removed === 1 ? 'version' : 'versions'}`
            : 'Loading…'}</span
        >
      </div>
    </div>
    <div class="dialog-warning" id="reset-history-warning">
      This cannot be undone. Existing versioned links may stop working or show different content.
      The main draft link stays unchanged.
    </div>
    <p class="dialog-error" id="reset-history-error" role="alert" hidden={!error}>{error}</p>
    <div class="dialog-actions">
      <Button variant="outline" id="reset-history-cancel" disabled={pending} onclick={onclose}
        >Cancel</Button
      ><Button
        variant="destructive"
        id="reset-history-confirm"
        disabled={!latest || pending || nothing}
        onclick={submit}>Reset and prune</Button
      >
    </div>
  </Dialog.Content>
</Dialog.Root>
