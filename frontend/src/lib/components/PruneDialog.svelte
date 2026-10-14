<script lang="ts">
  import { onDestroy } from 'svelte'
  import type { PurgeDraftResponse } from '$lib/generated/bindings'
  import type { Row } from '$lib/model'
  import { errorMessage, request } from '$lib/api'
  import * as Dialog from './ui/dialog'
  import { Button } from './ui/button'
  let {
    target,
    force,
    onclose,
    refresh,
    toast,
  }: {
    target: Row
    force: boolean
    onclose: () => void
    refresh: () => Promise<void>
    toast: (message: string, error?: boolean) => void
  } = $props()
  let pending = $state(false),
    error = $state(''),
    alive = true
  onDestroy(() => {
    alive = false
  })
  async function submit() {
    if (pending) return
    pending = true
    error = ''
    try {
      const result = await request<Partial<PurgeDraftResponse>>(
        `/api/drafts/${encodeURIComponent(target.draftId)}${force ? '?purge=true' : ''}`,
        { method: 'DELETE' },
      )
      if (alive) onclose()
      await refresh()
      toast(
        force
          ? result.cleanupComplete === false
            ? 'Draft deleted; storage cleanup pending. Run keryx storage gc.'
            : 'Draft permanently deleted.'
          : 'Draft pruned.',
        result.cleanupComplete === false,
      )
    } catch (cause) {
      if (alive) error = errorMessage(cause)
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
    id="prune-dialog"
    escapeKeydownBehavior={pending ? 'ignore' : 'close'}
    interactOutsideBehavior={pending ? 'ignore' : 'close'}
  >
    <div class="dialog-mark">!</div>
    <Dialog.Title id="dialog-title"
      >{force ? 'Permanently delete this draft?' : 'Prune this draft?'}</Dialog.Title
    >
    <Dialog.Description id="dialog-description"
      >{force
        ? 'Force prune removes the draft metadata, every stored version, and each HTML file.'
        : 'Prune stops serving the draft and removes it from the dashboard. Stored versions remain for housekeeping.'}</Dialog.Description
    >
    <div class="dialog-summary">
      <strong id="dialog-draft-title">{target.title}</strong><span id="dialog-draft-id"
        >{target.draftId}</span
      >
    </div>
    <div class="dialog-warning" id="dialog-warning">
      {force
        ? 'This cannot be undone. Public and versioned URLs will stop working.'
        : 'This is a soft delete. Run force prune or keryx purge later to remove the stored files.'}
    </div>
    <p class="dialog-error" role="alert">{error}</p>
    <div class="dialog-actions">
      <Button variant="outline" id="cancel-action" disabled={pending} onclick={onclose}
        >Cancel</Button
      ><Button
        variant={force ? 'destructive' : 'outline'}
        id="confirm-action"
        disabled={pending}
        onclick={submit}>{force ? 'Force prune' : 'Prune draft'}</Button
      >
    </div>
  </Dialog.Content>
</Dialog.Root>
