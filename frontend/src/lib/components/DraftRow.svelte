<script lang="ts">
  import { relativeFuture, relativeTime, repository, wakeLabel, type Row } from '$lib/model'
  import DraftActions, { type DraftAction } from './DraftActions.svelte'
  let {
    row,
    selectionMode,
    bulkChecked,
    onbulk,
    selected,
    management,
    pending,
    now,
    onselect,
    ontag,
    onaction,
  }: {
    row: Row
    selectionMode: boolean
    bulkChecked: boolean
    onbulk: (checked: boolean) => void
    selected: boolean
    management: boolean
    pending: boolean
    now: number
    onselect: () => void
    ontag: (id: string) => void
    onaction: (action: DraftAction, anchor?: HTMLElement) => void
  } = $props()
</script>

<tr
  class="draft-row"
  class:selected
  tabindex="0"
  aria-selected={selected}
  data-draft-id={row.draftId}
  data-version-count={row.versionCount}
  data-availability={row.availability}
  onclick={(event) => {
    if (
      !(event.target instanceof Element) ||
      !event.target.closest('a,button,input,label,details,summary')
    )
      onselect()
  }}
  onkeydown={(event) => {
    if (event.target === event.currentTarget && ['Enter', ' '].includes(event.key)) {
      event.preventDefault()
      onselect()
    }
  }}
>
  {#if selectionMode}<td class="bulk-checkbox-cell"
      ><input
        type="checkbox"
        aria-label={`Select ${row.title}`}
        checked={bulkChecked}
        onchange={(event) => onbulk(event.currentTarget.checked)}
      /></td
    >{/if}
  <td
    ><div class="draft-title">
      <a href={row.publicUrl} target="_blank" rel="noopener noreferrer">{row.title}</a>
    </div>
    <div class="draft-description">{row.description || 'No description supplied'}</div>
    {#if management}<div class="tag-chips">
        {#each row.tags as tag (tag.id)}<button
            class="tag-chip"
            type="button"
            data-filter-tag={tag.id}
            onclick={() => ontag(tag.id)}>{tag.name}</button
          >{/each}
      </div>{/if}
  </td>
  <td
    ><div class="source" class:missing={!repository(row)}>
      {management ? repository(row) || 'Provenance not recorded' : 'Protected'}
    </div>
    <div class="branch">
      {management ? row.latestGitBranch || 'Historic upload' : 'API key required'}
    </div></td
  >
  <td
    ><div class="updated" class:wake={row.availability === 'snoozed'} title={row.updatedAt}>
      {row.availability === 'snoozed'
        ? `Wakes ${relativeFuture(row.snoozedUntil, now)}`
        : relativeTime(row.updatedAt, now)}
    </div>
    <div class="version">
      {row.availability === 'snoozed'
        ? wakeLabel(row.snoozedUntil)
        : `v${row.latestVersionNumber ?? 0} · ${row.versionCount} ${row.versionCount === 1 ? 'version' : 'versions'}`}
    </div></td
  >
  {#if !selectionMode}<td class="row-action"
      ><DraftActions {row} {management} {pending} {onaction} /></td
    >{/if}
</tr>
