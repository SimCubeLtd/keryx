<script lang="ts">
  import type { Row } from '$lib/model'
  import { wakeLabel, relativeFuture } from '$lib/model'
  import * as Dialog from './ui/dialog'
  import { Button } from './ui/button'
  let {
    target,
    onclose,
    onsnooze,
  }: { target: Row; onclose: () => void; onsnooze: (id: string, until: string) => Promise<void> } =
    $props()
  function localInput(date: Date) {
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
  }
  function presetDate(preset: string) {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local arithmetic, not stored reactive state
    const date = new Date()
    if (preset === '1h') date.setHours(date.getHours() + 1)
    else if (preset === '4h') date.setHours(date.getHours() + 4)
    else if (preset === 'tomorrow') {
      date.setDate(date.getDate() + 1)
      date.setHours(9, 0, 0, 0)
    } else {
      date.setDate(date.getDate() + ((8 - date.getDay()) % 7 || 7))
      date.setHours(9, 0, 0, 0)
    }
    date.setSeconds(0, 0)
    return localInput(date)
  }
  let input = $state(presetDate('tomorrow')),
    preset = $state('tomorrow')
  const valid = $derived(Number.isFinite(Date.parse(input)) && Date.parse(input) > Date.now())
  const until = $derived(valid ? new Date(input).toISOString() : '')
</script>

<Dialog.Root
  open={true}
  onOpenChange={(open) => {
    if (!open) onclose()
  }}
>
  <Dialog.Content id="snooze-dialog">
    <Dialog.Title>Snooze this draft</Dialog.Title><Dialog.Description
      >Move it out of Active until a wake time. Public, raw, versioned, and PDF links keep working
      while it sleeps.</Dialog.Description
    >
    <div class="dialog-summary">
      <strong id="snooze-draft-title">{target.title}</strong><span id="snooze-draft-id"
        >{target.draftId}</span
      >
    </div>
    <div class="snooze-presets" aria-label="Wake time presets">
      {#each [['1h', 'In 1 hour'], ['4h', 'In 4 hours'], ['tomorrow', 'Tomorrow 09:00'], ['monday', 'Monday 09:00']] as [key, label] (key)}<button
          class="preset"
          type="button"
          data-preset={key}
          aria-pressed={preset === key}
          onclick={() => {
            if (key) {
              preset = key
              input = presetDate(key)
            }
          }}>{label}</button
        >{/each}
    </div>
    <label class="snooze-custom"
      >Custom wake time<input
        type="datetime-local"
        id="snooze-until"
        step="60"
        min={localInput(new Date())}
        bind:value={input}
        oninput={() => {
          preset = ''
        }}
      /></label
    >
    <p class="snooze-preview" id="snooze-preview">
      {valid
        ? `Wakes ${wakeLabel(until)} (${relativeFuture(until)})`
        : 'Pick a wake time in the future.'}
    </p>
    <div class="dialog-actions">
      <Button variant="outline" id="snooze-cancel" onclick={onclose}>Cancel</Button><Button
        id="snooze-confirm"
        disabled={!valid}
        onclick={() => {
          if (valid) {
            void onsnooze(target.draftId, until)
            onclose()
          }
        }}>Snooze</Button
      >
    </div>
  </Dialog.Content>
</Dialog.Root>
