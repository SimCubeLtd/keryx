<script lang="ts">
  import type { AvailabilityUpdate_Deserialize as AvailabilityUpdate } from '$lib/generated/bindings'
  import {
    canReset,
    relativeTime,
    repository,
    wakeLabel,
    relativeFuture,
    type Row,
  } from '$lib/model'
  import { Button } from './ui/button'
  import { Badge } from './ui/badge'
  import History from './History.svelte'
  let {
    row,
    management,
    now,
    pending,
    removing,
    ontag,
    onremove,
    onreset,
    onsnooze,
    onchange,
  }: {
    row: Row
    management: boolean
    now: number
    pending: boolean
    removing: string[]
    ontag: (anchor: HTMLButtonElement) => void
    onremove: (tagId: string) => void
    onreset: () => void
    onsnooze: () => void
    onchange: (change: AvailabilityUpdate) => void
  } = $props()
  const metadata = $derived([
    ['Origin', management ? row.repoHost || 'Not recorded' : 'Protected', 'detail-origin'],
    [
      'Repository',
      management ? repository(row) || 'Provenance not recorded' : 'Protected',
      'detail-repository',
    ],
    [
      'Latest branch',
      management ? row.latestGitBranch || 'Not recorded' : 'API key required',
      'detail-branch',
    ],
    ['Draft ID', row.draftId, 'detail-id'],
    ['Updated', relativeTime(row.updatedAt, now), 'detail-updated'],
    [
      'Current',
      `v${row.latestVersionNumber ?? 0} · ${row.versionCount} ${row.versionCount === 1 ? 'version' : 'versions'}`,
      'detail-version',
    ],
    [
      'Latest commit',
      management
        ? row.latestGitCommitSubject || row.latestGitCommitSha || 'Not recorded'
        : 'Protected',
      'detail-commit',
    ],
  ])
</script>

<div class="detail-content">
  <div class="detail-kicker">
    <Badge class={row.availability} id="detail-status"
      >{row.availability[0]?.toUpperCase()}{row.availability.slice(1)}</Badge
    > Selected draft
  </div>
  <h2 id="detail-title">{row.title}</h2>
  <p class="detail-copy" id="detail-description">{row.description || 'No description supplied.'}</p>
  <p
    class={`detail-availability ${row.availability}`}
    id="detail-availability"
    hidden={row.availability === 'active'}
  >
    {row.availability === 'snoozed'
      ? `Snoozed until ${wakeLabel(row.snoozedUntil)} (${relativeFuture(row.snoozedUntil, now)}). Public links keep working while it sleeps.`
      : 'Disabled. Public, raw, versioned, and PDF links return 404 until the draft is enabled.'}
  </p>
  <div class="detail-actions">
    <a
      class="button primary"
      id="detail-open"
      href={row.publicUrl}
      target="_blank"
      rel="noopener noreferrer"
      aria-disabled={row.disabled}>Open document ↗</a
    >
    {#if management}
      <a
        class="button"
        id="detail-pdf"
        href={`/api/drafts/${row.draftId}/pdf`}
        download
        aria-disabled={row.disabled}>Download PDF</a
      >
      <Button
        variant="outline"
        class="amber"
        id="action-snooze"
        hidden={row.availability !== 'active'}
        disabled={pending}
        onclick={onsnooze}>Snooze</Button
      >
      <Button
        variant="outline"
        class="amber"
        id="action-unsnooze"
        hidden={row.availability !== 'snoozed'}
        disabled={pending}
        onclick={() => onchange({ state: 'active' })}>Unsnooze</Button
      >
      <Button
        variant="outline"
        id="action-disable"
        hidden={row.disabled}
        disabled={pending}
        onclick={() => onchange({ state: 'disabled' })}>Disable</Button
      >
      <Button
        variant="outline"
        id="action-enable"
        hidden={!row.disabled}
        disabled={pending}
        onclick={() => onchange({ state: 'active' })}>Enable</Button
      >
    {:else}<span class="menu-note">Snooze, disable, and enable require the authenticated CLI.</span
      >{/if}
  </div>
  {#if management}<section class="detail-tags" aria-label="Tags">
      <h3>Tags</h3>
      <div id="detail-tags" class="tag-chips">
        {#each row.tags as tag (tag.id)}<button
            type="button"
            class="tag-chip"
            aria-label={`Remove tag ${tag.name}`}
            disabled={removing.includes(`${row.draftId}/${tag.id}`)}
            onclick={() => onremove(tag.id)}>{tag.name} ×</button
          >{/each}<Button
          variant="outline"
          id="add-tag"
          aria-label="Add tags"
          onclick={(event) => ontag(event.currentTarget)}>+ Add tags</Button
        >
      </div>
    </section>{/if}
  <div class="meta-grid">
    {#each metadata as [label, value, id] (id)}<div class="meta-cell">
        <div class="meta-key">{label}</div>
        <div class="meta-value" {id}>{value}</div>
      </div>{/each}
  </div>
  <div class="history-head">
    <h3>Version history</h3>
    {#if management}<Button
        variant="outline"
        id="reset-history"
        hidden={!canReset(row)}
        onclick={onreset}>Reset and prune version history…</Button
      >{/if}
  </div>
  <History draftId={row.draftId} {management} />
</div>
