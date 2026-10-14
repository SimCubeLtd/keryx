<script lang="ts">
  import { onDestroy, tick } from 'svelte'
  import { Popover } from 'bits-ui'
  import { Check, Search, X } from '@lucide/svelte'
  import type { Tag } from '$lib/generated/bindings'
  import { ApiError, assignTags, errorMessage } from '$lib/api'
  import { canonicalTag, type Row } from '$lib/model'
  import { Button } from './ui/button'
  import { Input } from './ui/input'

  let {
    targets,
    anchor,
    bulk,
    rows,
    catalogue,
    refresh,
    onconfirm,
    onclose,
  }: {
    targets: Row[]
    anchor: HTMLElement
    bulk: boolean
    rows: Row[]
    catalogue: Tag[]
    refresh: () => Promise<void>
    onconfirm: () => void
    onclose: () => void
  } = $props()
  let input = $state(''),
    debounced = $state(''),
    staged = $state<string[]>([])
  let waiting = $state(false),
    composing = $state(false),
    pending = $state(false),
    finished = $state(false),
    stopping = $state(false)
  let activeIndex = $state(0),
    error = $state('')
  let inputElement = $state<HTMLInputElement | null>(null)
  let frozenAnchor = $state<DOMRect | null>(null)
  type Result = {
    id: string
    title: string
    state: 'waiting' | 'saving' | 'assigned' | 'unchanged' | 'failed' | 'unknown' | 'unattempted'
    message?: string
  }
  let results = $state<Result[]>([])
  let alive = true,
    closing = false
  // Keep the anchor inside the viewport if live tags make the detail panel grow.
  const positionAnchor = $derived({
    contextElement: anchor,
    getBoundingClientRect: () => {
      const rect = frozenAnchor ?? anchor.getBoundingClientRect()
      const top = Math.max(12, Math.min(rect.top, window.innerHeight - rect.height - 12))
      return new DOMRect(rect.x, top, rect.width, rect.height)
    },
  })
  const current = $derived(
    targets.map((target) => rows.find((row) => row.draftId === target.draftId)),
  )
  const missing = $derived(current.some((row) => !row))
  const coverage = (name: string) =>
    current.filter((row) => row?.tags.some((tag) => tag.name === name)).length
  const additions = $derived(staged.filter((name) => coverage(name) < targets.length))
  const choices = $derived.by(() => {
    if (waiting || composing) return []
    const query = debounced.trim().replace(/ +/g, ' ').toLowerCase()
    const rank = (name: string) => (name === query ? 0 : name.startsWith(query) ? 1 : 2)
    const names = [...new Set([...catalogue.map((tag) => tag.name), ...staged])]
    const options = names
      .filter((name) => name.includes(query))
      .toSorted((a, b) => rank(a) - rank(b) || a.localeCompare(b))
      .map((name) => ({
        name,
        create: false,
        assigned: coverage(name) === targets.length,
        coverage: coverage(name),
      }))
    const canonical = canonicalTag(debounced)
    if (canonical && !names.includes(canonical))
      options.push({ name: canonical, create: true, assigned: false, coverage: 0 })
    return options
  })
  const active = $derived(
    choices[activeIndex]?.assigned ? choices.findIndex((choice) => !choice.assigned) : activeIndex,
  )
  $effect(() => {
    const value = input,
      composition = composing
    waiting = true
    activeIndex = 0
    if (composition) return
    const timer = setTimeout(() => {
      debounced = value
      waiting = false
    }, 150)
    return () => clearTimeout(timer)
  })
  onDestroy(() => {
    alive = false
    stopping = true
  })
  function close() {
    if (pending || closing) return
    closing = true
    const returnTo = anchor
    onclose()
    void tick().then(() =>
      requestAnimationFrame(() => {
        if (returnTo.isConnected) returnTo.focus({ preventScroll: true })
        else
          (
            document.getElementById('select-drafts') ??
            document.getElementById('add-tag') ??
            document.getElementById('draft-search')
          )?.focus()
      }),
    )
  }
  function toggle(index: number) {
    const choice = choices[index]
    if (!choice || choice.assigned || waiting || composing || pending) return
    staged = staged.includes(choice.name)
      ? staged.filter((name) => name !== choice.name)
      : [...staged, choice.name]
    error = ''
    if (choice.create) input = ''
    inputElement?.focus({ preventScroll: true })
  }
  function keydown(event: KeyboardEvent) {
    if (event.isComposing || composing) return
    if (event.key === 'Enter') {
      event.preventDefault()
      toggle(active)
      return
    }
    if (!['ArrowDown', 'ArrowUp'].includes(event.key) || waiting || pending) return
    event.preventDefault()
    for (let step = 1; step <= choices.length; step++) {
      const index =
        (active + (event.key === 'ArrowDown' ? step : -step) + choices.length) % choices.length
      if (!choices[index]?.assigned) {
        activeIndex = index
        break
      }
    }
  }
  async function apply() {
    if (!additions.length || pending || finished || missing || composing || waiting) return
    const names = [...additions]
    // Keep the popover in place when confirmation removes the bulk toolbar anchor.
    frozenAnchor = anchor.getBoundingClientRect()
    pending = true
    stopping = false
    error = ''
    results = targets.map((target) => ({
      id: target.draftId,
      title: target.title,
      state: 'waiting',
    }))
    if (bulk) onconfirm()
    for (const result of results) {
      if (!alive || stopping) break
      const live = rows.find((row) => row.draftId === result.id)
      if (live && names.every((name) => live.tags.some((tag) => tag.name === name))) {
        result.state = 'unchanged'
        continue
      }
      result.state = 'saving'
      try {
        await assignTags(result.id, names)
        if (!alive) return
        result.state = 'assigned'
      } catch (cause) {
        if (!alive) return
        result.state = cause instanceof ApiError ? 'failed' : 'unknown'
        result.message =
          cause instanceof ApiError
            ? errorMessage(cause)
            : 'Outcome unknown. Refresh and inspect the draft before trying again.'
        if (!(cause instanceof ApiError) || cause.status === 401 || cause.status === 403)
          stopping = true
      }
    }
    if (!alive) return
    for (const result of results) if (result.state === 'waiting') result.state = 'unattempted'
    await refresh().catch(() => {
      if (alive) error = 'The dashboard could not refresh. Inspect the result before trying again.'
    })
    if (!alive) return
    pending = false
    if (!bulk && results[0]?.state === 'assigned') {
      close()
      return
    }
    if (!bulk && results[0]?.state === 'failed') {
      error = results[0].message ?? 'Tags could not be assigned.'
      frozenAnchor = null
      await tick()
      inputElement?.focus({ preventScroll: true })
      return
    }
    finished = true
  }
</script>

<Popover.Root
  open={true}
  onOpenChange={(open) => {
    if (!open) close()
  }}
>
  <Popover.Portal>
    <Popover.Content
      id="tag-popover"
      class="tag-assignment-popover"
      role="dialog"
      aria-label="Add tags"
      customAnchor={positionAnchor}
      align="end"
      sideOffset={8}
      strategy="fixed"
      collisionPadding={12}
      escapeKeydownBehavior={pending ? 'ignore' : 'close'}
      interactOutsideBehavior={pending ? 'ignore' : 'close'}
      onOpenAutoFocus={(event) => {
        event.preventDefault()
        inputElement?.focus({ preventScroll: true })
      }}
      onCloseAutoFocus={(event) => event.preventDefault()}
    >
      <div class="tag-assignment-heading">
        <div>
          <strong>{finished ? 'Tag assignment results' : 'Add tags'}</strong><span id="tag-target"
            >{bulk
              ? `To ${targets.length} selected drafts · existing tags are kept`
              : `To ${targets[0]?.title ?? 'draft'}`}</span
          >
        </div>
        {#if !finished}<span>{additions.length} selected</span>{/if}
      </div>
      {#if !finished && !pending}
        <div class="tag-assignment-search">
          <Search size={15} /><Input
            id="tag-input"
            bind:ref={inputElement}
            bind:value={input}
            role="combobox"
            aria-label="Search or create a tag"
            aria-autocomplete="list"
            aria-controls="tag-suggestions"
            aria-expanded={choices.length > 0}
            aria-activedescendant={choices[active] ? `tag-option-${active}` : undefined}
            placeholder="Search or create a tag…"
            autocomplete="off"
            onkeydown={keydown}
            oncompositionstart={() => {
              composing = true
            }}
            oncompositionend={() => {
              composing = false
            }}
          />
        </div>
        {#if staged.length}<div class="tag-assignment-pending" aria-label="Pending tags">
            {#each staged as name (name)}<button
                type="button"
                class="tag-chip"
                aria-label={`Unstage ${name}`}
                onclick={() => {
                  staged = staged.filter((value) => value !== name)
                }}
                >{name}{#if !catalogue.some((tag) => tag.name === name)}<span class="new-tag-label"
                    >New</span
                  >{/if}<X size={11} /></button
              >{/each}
          </div>{/if}
        <div
          id="tag-suggestions"
          class="tag-assignment-options"
          role="listbox"
          aria-label="Tags"
          aria-multiselectable="true"
        >
          {#each choices as choice, index (choice.name)}
            <button
              id={`tag-option-${index}`}
              type="button"
              role="option"
              tabindex="-1"
              aria-selected={choice.assigned || staged.includes(choice.name)}
              aria-disabled={choice.assigned}
              class="tag-assignment-option"
              class:chosen={!choice.assigned && staged.includes(choice.name)}
              class:assigned={choice.assigned}
              class:active={active === index}
              disabled={choice.assigned}
              data-tag-name={choice.name}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => toggle(index)}
            >
              <span class="tag-assignment-check"
                >{#if choice.assigned || staged.includes(choice.name)}<Check size={12} />{/if}</span
              ><span class="tag-assignment-name"
                >{choice.create
                  ? `+ Create "${choice.name}"`
                  : choice.name}{#if !choice.create && !catalogue.some((tag) => tag.name === choice.name)}<span
                    class="new-tag-label">New</span
                  >{/if}</span
              ><span class="tag-assignment-coverage"
                >{choice.assigned
                  ? bulk
                    ? `Already on all ${targets.length}`
                    : 'Already assigned'
                  : bulk
                    ? `On ${choice.coverage} of ${targets.length}`
                    : ''}</span
              >
            </button>
          {/each}
        </div>
        <p class="tag-assignment-note" id="tag-status" role="status">
          {waiting || composing
            ? 'Updating suggestions…'
            : 'New tags are created only when you apply.'}
        </p>
        {#if missing}<p class="tag-assignment-error" role="alert">
            A selected draft is no longer available. Cancel and select drafts again.
          </p>{/if}
      {:else}
        <div class="tag-assignment-results" aria-live="polite">
          {#each results as result (result.id)}<div data-tag-result={result.state}>
              <strong>{result.title}</strong><span
                >{{
                  waiting: 'Waiting',
                  saving: 'Saving…',
                  assigned: 'Tags added',
                  unchanged: 'Already assigned',
                  failed: 'Failed',
                  unknown: 'Outcome unknown',
                  unattempted: 'Not attempted',
                }[result.state]}</span
              >{#if result.message}<p>{result.message}</p>{/if}
            </div>{/each}
        </div>
      {/if}
      {#if error}<p id="tag-error" class="tag-assignment-error" role="alert">{error}</p>{/if}
      <div class="tag-assignment-footer">
        {#if pending}<span role="status"
            >{results.filter((result) => !['waiting', 'saving'].includes(result.state)).length} of {targets.length}
            processed</span
          ><Button
            variant="outline"
            disabled={stopping}
            onclick={() => {
              stopping = true
            }}>Stop after current draft</Button
          >
        {:else if finished}<span
            >{results.filter((result) => ['assigned', 'unchanged'].includes(result.state)).length} of
            {targets.length} completed</span
          ><Button id="tag-done" onclick={close}>Done</Button>
        {:else}<Button
            variant="ghost"
            id="tag-clear"
            disabled={!staged.length}
            onclick={() => {
              staged = []
              input = ''
              error = ''
              inputElement?.focus({ preventScroll: true })
            }}>Clear</Button
          >
          <div>
            <Button variant="outline" id="tag-cancel" onclick={close}>Cancel</Button><Button
              id="tag-apply"
              disabled={!additions.length || missing || waiting || composing}
              onclick={() => {
                void apply()
              }}>Apply {additions.length} {additions.length === 1 ? 'tag' : 'tags'}</Button
            >
          </div>{/if}
      </div>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
