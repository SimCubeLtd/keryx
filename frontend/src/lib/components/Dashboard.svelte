<script lang="ts">
  import { onMount } from 'svelte'
  import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
  import { Search, Tags, Tag, ChevronDown } from '@lucide/svelte'
  import logo from '../../../../crates/keryx-render/assets/keryx-logo.svg'
  import type { AvailabilityUpdate_Deserialize as AvailabilityUpdate } from '$lib/generated/bindings'
  import {
    dashboardKey,
    fetchDashboard,
    errorMessage,
    removeTag,
    updateAvailability,
  } from '$lib/api'
  import {
    availability,
    compareRows,
    matches,
    repository,
    type Availability,
    type Row,
    type Sort,
  } from '$lib/model'
  import { connectDashboard, dashboardRefresh } from '$lib/realtime'
  import { Button } from './ui/button'
  import DraftRow from './DraftRow.svelte'
  import type { DraftAction } from './DraftActions.svelte'
  import DraftDetail from './DraftDetail.svelte'
  import TagAssignmentPopover from './TagAssignmentPopover.svelte'
  import ResetHistory from './ResetHistory.svelte'
  import PwaControls from './PwaControls.svelte'
  import ThemeControl from './ThemeControl.svelte'
  import RepositoryFilter from './RepositoryFilter.svelte'
  import AvailabilityDialog from './AvailabilityDialog.svelte'
  import PruneDialog from './PruneDialog.svelte'
  import BulkActionsDialog from './BulkActionsDialog.svelte'

  const client = useQueryClient()
  const query = createQuery(() => ({ queryKey: dashboardKey, queryFn: fetchDashboard }))
  const updates = dashboardRefresh(client)
  const refresh = updates.refresh
  const params = new URLSearchParams(location.search)
  const views: Availability[] = ['active', 'snoozed', 'disabled']
  const initialView = params.get('view'),
    initialSort = params.get('sort')
  let view = $state<Availability>(
    initialView === 'snoozed' || initialView === 'disabled' ? initialView : 'active',
  )
  let sort = $state<Sort>(
    initialSort === 'oldest' ||
      initialSort === 'title' ||
      initialSort === 'versions' ||
      initialSort === 'tag'
      ? initialSort
      : 'updated',
  )
  let selectedId = $state(params.get('draft'))
  let search = $state(params.get('search') ?? ''),
    repo = $state(params.get('repo') ?? '')
  let untagged = $state(params.get('untagged') === '1')
  let selectedTags = $state<string[]>(
    params.get('untagged') === '1' ? [] : [...new Set(params.getAll('tag'))],
  )
  let tagSearch = $state(''),
    tagMenuOpen = $state(false)
  let theme = $state(document.documentElement.dataset.theme ?? 'system')
  let initialized = $state(false),
    now = $state(Date.now())
  let tagSession = $state<{ targets: Row[]; anchor: HTMLElement; bulk: boolean } | null>(null)
  let resetTarget = $state<Row | null>(null),
    snoozeTarget = $state<Row | null>(null)
  let pruneTarget = $state<{ row: Row; force: boolean } | null>(null)
  let removing = $state<string[]>([])
  let toastMessage = $state(''),
    toastError = $state(false)
  let toastTimer: ReturnType<typeof setTimeout> | undefined
  let searchElement = $state<HTMLInputElement | undefined>(undefined)
  const management = $derived(query.data?.managementEnabled ?? false)
  const catalogue = $derived(query.data?.tags ?? [])
  const rows: Row[] = $derived(
    (query.data?.drafts ?? []).map((row) => ({
      ...row.summary,
      tags: row.tags,
      availability: availability(row.summary, now),
    })),
  )
  const baseRows = $derived(rows.filter((row) => matches(row, view, search, repo)))
  const visible = $derived(
    baseRows
      .filter((row) =>
        untagged
          ? !row.tags.length
          : selectedTags.every((id) => row.tags.some((tag) => tag.id === id)),
      )
      .toSorted(compareRows(sort)),
  )
  const selected = $derived(visible.find((row) => row.draftId === selectedId) ?? visible[0])
  const repositories = $derived(
    [...new Set([...rows.map(repository).filter(Boolean), ...(repo ? [repo] : [])])].sort(),
  )
  const counts = $derived(
    Object.fromEntries(
      views.map((value) => [value, rows.filter((row) => row.availability === value).length]),
    ),
  )
  const filterTags = $derived(
    catalogue.filter((tag) => tag.name.includes(tagSearch.trim().toLowerCase())),
  )
  let selectionMode = $state(false)
  let bulkIds = $state<string[]>([])
  let bulkTarget = $state<{ action: 'reset' | 'purge'; targets: Row[] } | null>(null)
  let previousScope = ''
  const selectionScope = $derived(
    JSON.stringify([view, search, repo, untagged, [...selectedTags].sort()]),
  )
  const bulkRows = $derived(visible.filter((row) => bulkIds.includes(row.draftId)))
  $effect(() => {
    if (previousScope !== selectionScope) {
      previousScope = selectionScope
      bulkIds = []
    }
  })
  $effect(() => {
    const remaining = bulkIds.filter((id) => visible.some((row) => row.draftId === id))
    if (remaining.length !== bulkIds.length) bulkIds = remaining
    if (!management && (selectionMode || bulkIds.length)) {
      selectionMode = false
      bulkIds = []
    }
  })
  function toggleBulk(id: string, checked: boolean) {
    bulkIds = checked ? [...bulkIds, id] : bulkIds.filter((value) => value !== id)
  }
  function openBulk(action: 'reset' | 'purge') {
    if (bulkRows.length)
      bulkTarget = { action, targets: bulkRows.map((row) => ({ ...row, tags: [...row.tags] })) }
  }
  const hints = {
    active: 'Publish one with keryx upload ./plan.html',
    snoozed: 'Snooze a draft to park it until a wake time.',
    disabled: 'Disable a draft to stop serving it.',
  }

  function toast(message: string, error = false) {
    toastMessage = message
    toastError = error
    clearTimeout(toastTimer)
    toastTimer = setTimeout(() => {
      toastMessage = ''
    }, 4200)
  }
  $effect(() => {
    if (!query.data || initialized) return
    if (!query.data.managementEnabled) {
      selectedTags = []
      untagged = false
      repo = ''
      if (sort === 'tag') sort = 'updated'
    }
    const linked = rows.find((row) => row.draftId === params.get('draft'))
    if (linked) view = linked.availability
    initialized = true
  })
  $effect(() => {
    if (!initialized) return
    const next = new URLSearchParams()
    if (selected) next.set('draft', selected.draftId)
    next.set('view', view)
    if (search) next.set('search', search)
    if (repo) next.set('repo', repo)
    if (sort !== 'updated') next.set('sort', sort)
    selectedTags.forEach((id) => next.append('tag', id))
    if (untagged) next.set('untagged', '1')
    history.replaceState(null, '', '/?' + next.toString())
  })
  $effect(() => {
    if (
      tagSession &&
      !tagSession.bulk &&
      query.data &&
      !rows.some((row) => row.draftId === tagSession?.targets[0]?.draftId)
    ) {
      tagSession = null
      toast('The draft is no longer available for tagging.', true)
    }
  })
  $effect(() => {
    const nextWake = Math.min(
      ...rows
        .filter((row) => row.availability === 'snoozed')
        .map((row) => Date.parse(row.snoozedUntil ?? '')),
    )
    if (!Number.isFinite(nextWake)) return
    const timer = setTimeout(
      () => {
        const time = Date.now()
        const woke = rows.filter(
          (row) => row.availability === 'snoozed' && Date.parse(row.snoozedUntil ?? '') <= time,
        )
        now = time
        if (woke.length === 1) toast(`${woke[0]?.title} woke and is active again.`)
        else if (woke.length > 1) toast(`${woke.length} drafts woke and are active again.`)
      },
      Math.min(Math.max(nextWake - Date.now(), 0) + 100, 86400000),
    )
    return () => clearTimeout(timer)
  })
  $effect(() => {
    document.documentElement.dataset.theme = theme
    document.documentElement.classList.toggle(
      'dark',
      theme === 'dark' ||
        (theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches),
    )
    try {
      localStorage.setItem('keryx-theme', theme)
    } catch {
      /* Browser storage may be disabled. */
    }
  })
  onMount(() => {
    const disconnect = connectDashboard(refresh)
    const visibility = () => {
      if (document.visibilityState === 'visible') {
        now = Date.now()
        void refresh()
      }
    }
    const media = matchMedia('(prefers-color-scheme: dark)')
    const systemTheme = () => {
      if (theme === 'system') document.documentElement.classList.toggle('dark', media.matches)
    }
    media.addEventListener('change', systemTheme)
    document.addEventListener('visibilitychange', visibility)
    return () => {
      media.removeEventListener('change', systemTheme)
      disconnect()
      updates.close()
      document.removeEventListener('visibilitychange', visibility)
      clearTimeout(toastTimer)
    }
  })
  function clearTags() {
    selectedTags = []
    untagged = false
  }
  function chooseTag(id: string) {
    untagged = false
    if (!selectedTags.includes(id)) selectedTags = [...selectedTags, id]
  }
  function toggleTag(id: string, checked: boolean) {
    untagged = false
    selectedTags = checked ? [...selectedTags, id] : selectedTags.filter((tag) => tag !== id)
  }
  function closeTagFilter() {
    tagMenuOpen = false
    document.getElementById('tag-filter-summary')?.focus()
  }
  function openTags(targets: Row[], anchor: HTMLElement, bulk = false) {
    tagSession = { targets: targets.map((row) => ({ ...row, tags: [...row.tags] })), anchor, bulk }
  }
  function openRowAction(
    row: Row,
    action: DraftAction,
    anchor: HTMLElement | undefined = undefined,
  ) {
    if (action === 'tags') {
      if (anchor) openTags([row], anchor)
    } else if (action === 'snooze') snoozeTarget = row
    else if (action === 'enable' || action === 'unsnooze')
      void changeAvailability(row.draftId, { state: 'active' })
    else if (action === 'disable') void changeAvailability(row.draftId, { state: 'disabled' })
    else if (action === 'reset') resetTarget = row
    else pruneTarget = { row, force: action === 'purge' }
  }
  async function deleteTag(row: Row, id: string) {
    const key = `${row.draftId}/${id}`
    if (removing.includes(key)) return
    removing = [...removing, key]
    try {
      await removeTag(row.draftId, id)
      await refresh()
    } catch (cause) {
      toast(errorMessage(cause), true)
    } finally {
      removing = removing.filter((value) => value !== key)
    }
  }
  const availabilityMutation = createMutation(() => ({
    mutationFn: ({ id, change }: { id: string; change: AvailabilityUpdate }) =>
      updateAvailability(id, change),
    onSuccess: refresh,
  }))
  async function changeAvailability(id: string, change: AvailabilityUpdate) {
    try {
      await availabilityMutation.mutateAsync({ id, change })
      toast(
        change.state === 'snoozed'
          ? 'Draft snoozed. Public links keep working.'
          : change.state === 'disabled'
            ? 'Draft disabled. Public links now return 404.'
            : 'Draft is active again.',
      )
    } catch (cause) {
      toast(errorMessage(cause), true)
    }
  }
  function keyboard(event: KeyboardEvent) {
    if (
      event.key === 'Escape' &&
      tagMenuOpen &&
      event.target instanceof Element &&
      event.target.closest('#tag-filter')
    ) {
      event.preventDefault()
      closeTagFilter()
      return
    }
    if (
      event.key === '/' &&
      !event.ctrlKey &&
      !event.metaKey &&
      event.target instanceof HTMLElement &&
      !event.target.closest('input, textarea, select, [contenteditable], [role="dialog"]')
    ) {
      event.preventDefault()
      searchElement?.focus()
    }
  }
  function clickOutside(event: MouseEvent) {
    if (!(event.target instanceof Element)) return
    if (!event.target.closest('#tag-filter')) tagMenuOpen = false
    const target = event.target
    document.querySelectorAll<HTMLDetailsElement>('.notify-menu[open]').forEach((menu) => {
      if (!menu.contains(target)) menu.open = false
    })
  }
</script>

<svelte:document onkeydown={keyboard} onclick={clickOutside} />
<header class="topbar">
  <div class="brand">
    <span class="brand-mark"><img src={logo} alt="" width="25" height="25" /></span>Keryx
    {#if query.data}<span class="app-version" aria-label={`Version ${query.data.version}`}
        >v{query.data.version}</span
      >{/if}
  </div>
  <div class="topbar-tools">
    {#if query.data && !management}<span class="management-mode"
        >Public view · management via authenticated CLI</span
      >{/if}
    <PwaControls {management} {toast} /><a class="health" href="/healthz">Server healthy</a>
    <ThemeControl bind:value={theme} />
  </div>
</header>
<main class="dashboard">
  <section class="workspace" class:bulk-mode={selectionMode}>
    <div class="workspace-head">
      <div class="head-row">
        <div>
          <h1>Drafts</h1>
          <p id="result-count">
            {visible.length === counts[view] ? '' : `${visible.length} of `}{counts[view] ?? 0}
            {view}
          </p>
        </div>
        <label class="search"
          ><Search size={16} /><input
            id="draft-search"
            bind:this={searchElement}
            type="search"
            placeholder={`Search ${view} drafts…`}
            aria-label="Search drafts"
            bind:value={search}
          /><span class="kbd">/</span></label
        >
      </div>
      <div class="filters">
        <div class="tabs" role="tablist" aria-label="Availability">
          {#each views as value (value)}<button
              class={`tab ${value === 'snoozed' ? 'snooze' : value === 'disabled' ? 'off' : ''}`}
              type="button"
              role="tab"
              data-view={value}
              aria-selected={view === value}
              onclick={() => {
                view = value
              }}
              >{value[0]?.toUpperCase()}{value.slice(1)}
              <span class="count" data-count={value}>{counts[value] ?? 0}</span></button
            >{/each}
        </div>
        {#if management}
          <RepositoryFilter
            {repositories}
            rows={rows.filter((row) => row.availability === view)}
            {view}
            bind:value={repo}
          />
          <div class="tag-bulk-controls">
            <details id="tag-filter" class="tag-filter" bind:open={tagMenuOpen}>
              <summary
                id="tag-filter-summary"
                aria-label={untagged
                  ? 'Tags: untagged'
                  : selectedTags.length
                    ? `Tags: ${selectedTags.length} selected`
                    : 'Tags: all'}
              >
                <Tags size={14} /><span>Tags</span>
                {#if selectedTags.length || untagged}<span class="filter-count"
                    >{untagged ? 'Untagged' : selectedTags.length}</span
                  >{/if}
                <ChevronDown size={13} class="filter-chevron" />
              </summary>
              <div class="tag-filter-menu">
                <div class="tag-filter-heading">
                  <div>
                    <strong>Filter by tags</strong><span class="tag-filter-description"
                      >Show drafts with every selected tag.</span
                    >
                  </div>
                  <span class="tag-selection-count"
                    >{untagged ? 'Untagged' : `${selectedTags.length} selected`}</span
                  >
                </div>
                <label class="tag-filter-search" for="tag-filter-search"
                  ><Search size={15} /><input
                    id="tag-filter-search"
                    aria-label="Find tags"
                    placeholder="Find tags…"
                    type="search"
                    bind:value={tagSearch}
                  /></label
                >
                <div id="tag-filter-options">
                  {#each filterTags as tag (tag.id)}<label
                      class="tag-filter-option"
                      class:chosen={selectedTags.includes(tag.id)}
                    >
                      <input
                        type="checkbox"
                        data-tag-choice={tag.id}
                        checked={selectedTags.includes(tag.id)}
                        onchange={(event) => toggleTag(tag.id, event.currentTarget.checked)}
                      />
                      <Tag size={14} /><span class="tag-filter-name">{tag.name}</span>
                      <span class="tag-filter-count"
                        >{baseRows.filter((row) => row.tags.some((item) => item.id === tag.id))
                          .length}</span
                      >
                    </label>
                  {:else}<p class="tag-filter-empty">No matching tags</p>{/each}
                </div>
                <div class="tag-filter-untagged">
                  <label class="tag-filter-option" class:chosen={untagged}>
                    <input
                      id="tag-untagged"
                      type="checkbox"
                      checked={untagged}
                      onchange={(event) => {
                        untagged = event.currentTarget.checked
                        selectedTags = []
                      }}
                    />
                    <span class="tag-filter-name">Untagged only</span>
                    <span id="untagged-count" class="tag-filter-count"
                      >{baseRows.filter((row) => !row.tags.length).length}</span
                    >
                  </label>
                </div>
                <div class="tag-filter-footer">
                  <Button variant="ghost" data-clear-tags onclick={clearTags}>Clear all</Button>
                  <Button id="tag-filter-done" onclick={closeTagFilter}>Done</Button>
                </div>
              </div>
            </details>

            <Button
              variant="outline"
              id="select-drafts"
              class="select-drafts"
              aria-pressed={selectionMode}
              onclick={() => {
                selectionMode = !selectionMode
                bulkIds = []
              }}>{selectionMode ? 'Cancel bulk actions' : 'Bulk Actions'}</Button
            >
          </div>
        {:else}<span class="protected-note"
            >Git provenance and management require the authenticated CLI</span
          >{/if}
        <div class="filters-tail">
          <label class="sort"
            >Sort<select id="draft-sort" aria-label="Sort drafts" bind:value={sort}
              ><option value="updated">Recently updated</option><option value="oldest"
                >Oldest updated</option
              ><option value="title">Title</option><option value="versions">Most versions</option
              >{#if management}<option value="tag">Tag A–Z</option>{/if}</select
            ></label
          >
        </div>
      </div>
      {#if management}
        <div id="tag-selections" class="tag-chips">
          {#each selectedTags as id (id)}<button
              class="tag-chip"
              type="button"
              aria-label={`Clear filter ${catalogue.find((tag) => tag.id === id)?.name ?? id}`}
              onclick={() => {
                selectedTags = selectedTags.filter((tag) => tag !== id)
              }}>{catalogue.find((tag) => tag.id === id)?.name ?? `Missing tag ${id}`} ×</button
            >{/each}{#if untagged}<button type="button" class="tag-chip" onclick={clearTags}
              >Untagged ×</button
            >{/if}{#if selectedTags.length || untagged}<button
              type="button"
              class="tag-chip"
              onclick={clearTags}>Clear all</button
            >{/if}
        </div>
        <p id="tag-sort-help" hidden={sort !== 'tag'}>
          Sorts by first alphabetical tag, then title. Untagged drafts come last.
        </p>
        <Button
          variant="outline"
          id="clear-empty-tags"
          data-clear-tags
          hidden={visible.length > 0 || (!selectedTags.length && !untagged)}
          onclick={clearTags}>Clear tag filter</Button
        >
      {/if}
      {#if management && selectionMode}
        <div class="bulk-action-bar" aria-label="Bulk actions">
          <span role="status" aria-live="polite"><strong>{bulkRows.length} selected</strong></span>
          <span class="bulk-version-count"
            >{bulkRows.reduce((sum, row) => sum + row.versionCount, 0)}
            {bulkRows.reduce((sum, row) => sum + row.versionCount, 0) === 1
              ? 'version'
              : 'versions'}</span
          >
          <div class="bulk-action-buttons">
            <Button
              variant="outline"
              id="bulk-tags"
              disabled={!bulkRows.length}
              onclick={(event) => openTags(bulkRows, event.currentTarget, true)}>Add tags…</Button
            >
            <Button
              variant="outline"
              id="bulk-reset"
              disabled={!bulkRows.length}
              onclick={() => openBulk('reset')}>Reset &amp; prune history…</Button
            ><Button
              variant="destructive-outline"
              id="bulk-delete"
              disabled={!bulkRows.length}
              onclick={() => openBulk('purge')}>Delete permanently…</Button
            >
          </div>
          <Button
            variant="ghost"
            disabled={!bulkRows.length}
            onclick={() => {
              bulkIds = []
            }}>Clear selection</Button
          >
        </div>
      {/if}
    </div>
    {#if query.isPending}<p class="dashboard-message" role="status">Loading drafts…</p>{/if}
    {#if query.error}<p class="dashboard-message" role="alert">
        Live refresh failed: {query.error.message}
        <Button variant="outline" onclick={() => query.refetch()}>Retry</Button>
      </p>{/if}
    <div class="split">
      <div class="master">
        <div class="table-wrap">
          <table
            class="draft-table"
            class:bulk-selecting={selectionMode}
            role="grid"
            aria-label="Drafts"
          >
            <colgroup
              >{#if selectionMode}<col class="bulk-checkbox-col" />{/if}<col /><col /><col
              />{#if !selectionMode}<col />{/if}</colgroup
            ><thead
              ><tr
                >{#if selectionMode}<th class="bulk-checkbox-cell"
                    ><input
                      type="checkbox"
                      aria-label="Select all filtered drafts"
                      checked={visible.length > 0 && bulkRows.length === visible.length}
                      indeterminate={bulkRows.length > 0 && bulkRows.length < visible.length}
                      disabled={!visible.length}
                      onchange={(event) => {
                        bulkIds = event.currentTarget.checked
                          ? visible.map((row) => row.draftId)
                          : []
                      }}
                    /></th
                  >{/if}<th>Document</th><th>Source</th><th id="time-column"
                  >{view === 'snoozed' ? 'Wakes' : 'Updated'}</th
                >{#if !selectionMode}<th></th>{/if}</tr
              ></thead
            ><tbody id="draft-rows">
              {#each visible as row (row.draftId)}<DraftRow
                  {row}
                  {selectionMode}
                  bulkChecked={bulkIds.includes(row.draftId)}
                  onbulk={(checked) => toggleBulk(row.draftId, checked)}
                  selected={selected?.draftId === row.draftId}
                  {management}
                  pending={availabilityMutation.isPending}
                  {now}
                  onselect={() => {
                    selectedId = row.draftId
                  }}
                  ontag={chooseTag}
                  onaction={(action, anchor) => openRowAction(row, action, anchor)}
                />{/each}
            </tbody>
          </table>
        </div>
        <div
          class="empty-results"
          id="empty-results"
          hidden={visible.length > 0 || query.isPending}
        >
          <strong id="empty-title"
            >{counts[view] ? 'No matching drafts' : `No ${view} drafts`}</strong
          ><span id="empty-hint"
            >{counts[view] ? 'Adjust the search or repository filter.' : hints[view]}</span
          >
        </div>
      </div>
      <aside
        class="detail"
        id="draft-detail"
        class:empty={!selected}
        aria-label="Selected draft details"
      >
        {#if selected}<DraftDetail
            row={selected}
            {management}
            {now}
            pending={availabilityMutation.isPending}
            {removing}
            ontag={(anchor) => {
              if (selected) openTags([selected], anchor)
            }}
            onremove={(id) => {
              if (selected) void deleteTag(selected, id)
            }}
            onreset={() => {
              resetTarget = selected ?? null
            }}
            onsnooze={() => {
              snoozeTarget = selected ?? null
            }}
            onchange={(change) => {
              if (selected) void changeAvailability(selected.draftId, change)
            }}
          />
        {:else}<div class="detail-empty">
            <h2>{rows.length ? 'No matching drafts' : 'No drafts yet'}</h2>
            <p>
              {rows.length
                ? 'Adjust the search or pick another tab.'
                : 'Publish one with keryx upload ./plan.html.'}
            </p>
          </div>{/if}
      </aside>
    </div>
  </section>
</main>
{#if bulkTarget}<BulkActionsDialog
    targets={bulkTarget.targets}
    action={bulkTarget.action}
    {rows}
    {refresh}
    onconfirm={() => {
      selectionMode = false
      bulkIds = []
    }}
    onclose={() => {
      bulkTarget = null
    }}
  />{/if}
{#if tagSession}<TagAssignmentPopover
    targets={tagSession.targets}
    anchor={tagSession.anchor}
    bulk={tagSession.bulk}
    {rows}
    {catalogue}
    {refresh}
    onconfirm={() => {
      selectionMode = false
      bulkIds = []
    }}
    onclose={() => {
      tagSession = null
    }}
  />{/if}
{#if resetTarget}<ResetHistory
    target={resetTarget}
    {refresh}
    {toast}
    onclose={() => {
      resetTarget = null
    }}
  />{/if}
{#if snoozeTarget}<AvailabilityDialog
    target={snoozeTarget}
    onclose={() => {
      snoozeTarget = null
    }}
    onsnooze={(id, until) => changeAvailability(id, { state: 'snoozed', until })}
  />{/if}
{#if pruneTarget}<PruneDialog
    target={pruneTarget.row}
    force={pruneTarget.force}
    {refresh}
    {toast}
    onclose={() => {
      pruneTarget = null
    }}
  />{/if}
<div class="toast" id="toast" role="status" class:error={toastError} hidden={!toastMessage}>
  {toastMessage}
</div>
