<script module lang="ts">
  export type DraftAction =
    | 'tags'
    | 'snooze'
    | 'unsnooze'
    | 'disable'
    | 'enable'
    | 'reset'
    | 'prune'
    | 'purge'
</script>

<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import {
    ExternalLink,
    Code,
    FileDown,
    Download,
    AlarmClock,
    Ban,
    CircleCheck,
    TimerOff,
    Tags,
    History,
    Archive,
    Trash2,
    Ellipsis,
  } from '@lucide/svelte'
  import { canReset, type Row } from '$lib/model'
  import { Badge } from './ui/badge'
  let {
    row,
    management,
    pending,
    onaction,
  }: {
    row: Row
    management: boolean
    pending: boolean
    onaction: (action: DraftAction, anchor?: HTMLElement) => void
  } = $props()
  let trigger = $state<HTMLButtonElement | null>(null)
  let openingDialog = false
  function act(action: DraftAction) {
    openingDialog = ['tags', 'snooze', 'reset', 'prune', 'purge'].includes(action)
    onaction(action, trigger ?? undefined)
  }
</script>

{#snippet linkItem(
  Icon: typeof ExternalLink,
  label: string,
  href: string,
  download: string | boolean | undefined = undefined,
  external = false,
)}
  <DropdownMenu.Item disabled={row.disabled} textValue={label}>
    {#snippet child({ props })}
      <a
        {...props}
        class="draft-action-item"
        href={row.disabled ? undefined : href}
        {download}
        target={external ? '_blank' : undefined}
        rel={external ? 'noopener noreferrer' : undefined}
      >
        <Icon size={16} /><span class="draft-action-copy"
          ><span class="draft-action-label">{label}</span></span
        >
      </a>
    {/snippet}
  </DropdownMenu.Item>
{/snippet}
{#snippet actionItem(
  action: DraftAction,
  Icon: typeof ExternalLink,
  label: string,
  help = '',
  className = '',
)}
  <DropdownMenu.Item
    class={`draft-action-item ${className}`}
    disabled={pending}
    data-action={action === 'reset' ? 'reset-history' : action}
    textValue={label}
    onSelect={() => act(action)}
  >
    <Icon size={16} /><span class="draft-action-copy"
      ><span class="draft-action-label">{label}</span>{#if help}<span class="draft-action-help"
          >{help}</span
        >{/if}</span
    >
  </DropdownMenu.Item>
{/snippet}
<DropdownMenu.Root
  onOpenChange={(open) => {
    if (open) openingDialog = false
  }}
>
  <DropdownMenu.Trigger
    bind:ref={trigger}
    class="draft-actions-trigger"
    aria-label={`Actions for ${row.title}`}><Ellipsis size={18} /></DropdownMenu.Trigger
  >
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      class="draft-actions-menu"
      align="end"
      sideOffset={8}
      collisionPadding={12}
      aria-label={`Actions for ${row.title}`}
      onCloseAutoFocus={(event) => {
        if (openingDialog) event.preventDefault()
      }}
    >
      <div class="draft-actions-heading">
        <strong>Draft actions</strong><Badge class={row.availability}
          >{row.availability[0]?.toUpperCase()}{row.availability.slice(1)}</Badge
        ><span title={row.title}>{row.title} · v{row.latestVersionNumber ?? 0}</span>
      </div>
      <DropdownMenu.Group class="draft-actions-group">
        <DropdownMenu.GroupHeading class="draft-actions-group-heading"
          >Open &amp; export</DropdownMenu.GroupHeading
        >
        {@render linkItem(ExternalLink, 'Open document', row.publicUrl, undefined, true)}
        {@render linkItem(Code, 'View raw HTML', row.rawUrl)}
        {#if management}{@render linkItem(
            FileDown,
            'Download PDF',
            `/api/drafts/${row.draftId}/pdf`,
            true,
          )}{/if}
        {@render linkItem(
          Download,
          'Download HTML',
          row.rawUrl,
          `keryx-${row.draftId}-v${row.latestVersionNumber ?? 0}.html`,
        )}
      </DropdownMenu.Group>
      {#if management}
        <DropdownMenu.Group class="draft-actions-group">
          <DropdownMenu.GroupHeading class="draft-actions-group-heading"
            >Manage</DropdownMenu.GroupHeading
          >
          {@render actionItem('tags', Tags, 'Add tags…')}
          {#if row.availability === 'active'}{@render actionItem(
              'snooze',
              AlarmClock,
              'Snooze…',
              '',
              'amber',
            )}
          {:else if row.availability === 'snoozed'}{@render actionItem(
              'unsnooze',
              TimerOff,
              'Unsnooze',
              '',
              'amber',
            )}{/if}
          {#if row.disabled}{@render actionItem(
              'enable',
              CircleCheck,
              'Enable',
              'Restore public access',
            )}
          {:else}{@render actionItem('disable', Ban, 'Disable', 'Pause public access')}{/if}
        </DropdownMenu.Group>
        <DropdownMenu.Group class="draft-actions-group">
          <DropdownMenu.GroupHeading class="draft-actions-group-heading"
            >History &amp; removal</DropdownMenu.GroupHeading
          >
          {#if canReset(row)}{@render actionItem(
              'reset',
              History,
              'Reset version history…',
              'Keep latest; restart at v1',
            )}{/if}
          {@render actionItem(
            'prune',
            Archive,
            'Prune draft…',
            'Stop serving; retain stored versions',
          )}
          {@render actionItem(
            'purge',
            Trash2,
            'Delete permanently…',
            'Delete draft and every version',
            'danger',
          )}
        </DropdownMenu.Group>
      {:else}<p class="draft-actions-note">
          PDF publishing and management require the authenticated CLI.
        </p>{/if}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
