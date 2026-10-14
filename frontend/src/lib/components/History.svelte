<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import { fetchHistory, historyKey } from '$lib/api'
  import { relativeTime, fileSize } from '$lib/model'
  import { Button } from './ui/button'
  let { draftId, management }: { draftId: string; management: boolean } = $props()
  let expanded = $state(false)
  const query = createQuery(() => ({
    queryKey: historyKey(draftId),
    queryFn: ({ signal }) => fetchHistory(draftId, signal),
    enabled: management,
  }))
  const versions = $derived(query.data?.versions ?? [])
  const visible = $derived(expanded ? versions : versions.slice(0, 8))
  $effect(() => {
    void draftId
    expanded = false
  })
</script>

<span class="history-state" id="history-state"
  >{!management
    ? 'Use authenticated CLI'
    : query.error
      ? query.error.message
      : query.isPending
        ? 'Loading…'
        : `${versions.length} ${versions.length === 1 ? 'version' : 'versions'}`}</span
>
<div id="version-history">
  {#if management}
    {#each visible as version (version.id)}
      <a
        class="history-item"
        href={`/d/${encodeURIComponent(draftId)}/v/${version.versionNumber}`}
        target="_blank"
        rel="noopener noreferrer"
        aria-label={`Open version ${version.versionNumber} in a new tab`}
      >
        <span class="vbadge">v{version.versionNumber}</span>
        <div>
          <div class="commit">
            {version.gitCommitSubject || version.originalFilename || 'Published version'}
          </div>
          <div class="commit-meta">
            {version.repoOrg && version.repoName
              ? `${version.repoOrg}/${version.repoName}`
              : 'provenance not recorded'} · {version.gitBranch || 'branch not recorded'} · {version.gitCommitSha?.slice(
              0,
              8,
            ) || 'no commit'} · {fileSize(version.fileSize)}
          </div>
        </div>
        <time class="history-time">{relativeTime(version.createdAt)}</time><span
          class="history-open">↗</span
        >
      </a>
    {/each}
    {#if visible.length < versions.length}<div class="history-more">
        <div class="history-count">
          <strong>Showing {visible.length} of {versions.length}</strong><span
            >Newest versions first</span
          >
        </div>
        <Button
          variant="outline"
          class="history-more-button"
          onclick={() => {
            expanded = true
          }}
          >Load {versions.length - visible.length} older {versions.length - visible.length === 1
            ? 'version'
            : 'versions'}</Button
        >
      </div>{/if}
  {/if}
</div>
