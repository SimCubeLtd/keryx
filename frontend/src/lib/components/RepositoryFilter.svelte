<script lang="ts">
  import { Popover } from 'bits-ui'
  import { SvelteMap } from 'svelte/reactivity'
  import { Check, ChevronDown, Search } from '@lucide/svelte'
  import { repository, type Availability, type Row } from '$lib/model'

  let {
    repositories,
    rows,
    view,
    value = $bindable(''),
  }: {
    repositories: string[]
    rows: Row[]
    view: Availability
    value?: string
  } = $props()
  let open = $state(false)
  let search = $state('')
  const entries = $derived(
    repositories.map((path) => {
      const slash = path.indexOf('/')
      return { path, owner: path.slice(0, slash), name: path.slice(slash + 1) }
    }),
  )
  const filtered = $derived(
    entries.filter(({ path }) =>
      path.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()),
    ),
  )
  const groups = $derived(
    [...new Set(filtered.map(({ owner }) => owner))].map((owner) => ({
      owner,
      repositories: filtered.filter((entry) => entry.owner === owner),
    })),
  )
  const counts = $derived.by(() => {
    const result = new SvelteMap<string, number>()
    for (const row of rows) {
      const path = repository(row)
      result.set(path, (result.get(path) ?? 0) + 1)
    }
    return result
  })

  function select(path: string) {
    value = path
    open = false
  }
</script>

<Popover.Root
  bind:open
  onOpenChange={() => {
    search = ''
  }}
>
  <Popover.Trigger
    id="repo-filter"
    class="repo-filter"
    aria-label={`Filter by repository: ${value || 'All repositories'}`}
    aria-haspopup="dialog"
    title={value || 'All repositories'}
  >
    <span class="repository-trigger-label">{value || 'All repositories'}</span><ChevronDown
      size={13}
    />
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      class="repository-menu"
      role="dialog"
      align="start"
      sideOffset={8}
      collisionPadding={16}
      aria-label="Filter by repository"
    >
      <div class="repository-heading">
        <strong>Repositories</strong><span>{repositories.length} repositories</span>
      </div>
      <label class="repository-search"
        ><Search size={15} /><input
          type="search"
          aria-label="Find a repository"
          placeholder="Find a repository…"
          bind:value={search}
        /></label
      >
      <button
        type="button"
        class="repository-option repository-all"
        class:chosen={!value}
        aria-pressed={!value}
        onclick={() => select('')}
      >
        <span class="repository-check"
          >{#if !value}<Check size={14} />{/if}</span
        ><span class="repository-name">All repositories</span><span class="repository-count"
          >{rows.length}</span
        >
      </button>
      <div class="repository-options">
        {#each groups as group (group.owner)}
          <section class="repository-group" aria-label={group.owner}>
            <h3>{group.owner}</h3>
            {#each group.repositories as entry (entry.path)}
              <button
                type="button"
                class="repository-option"
                class:chosen={value === entry.path}
                aria-pressed={value === entry.path}
                aria-label={entry.path}
                onclick={() => select(entry.path)}
              >
                <span class="repository-check"
                  >{#if value === entry.path}<Check size={14} />{/if}</span
                ><span class="repository-name">{entry.name}</span><span class="repository-count"
                  >{counts.get(entry.path) ?? 0}</span
                >
              </button>
            {/each}
          </section>
        {:else}<p class="repository-empty">No matching repositories</p>{/each}
      </div>
      <p class="repository-footer">Counts for {view} drafts · Select one repository</p>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
