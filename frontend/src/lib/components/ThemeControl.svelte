<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { Check, Monitor, Moon, Sun } from '@lucide/svelte'
  let { value = $bindable('system') }: { value?: string } = $props()
  const choices = [
    { value: 'light', label: 'Light', icon: Sun },
    { value: 'dark', label: 'Dark', icon: Moon },
    { value: 'system', label: 'System', icon: Monitor },
  ]
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger
    id="theme-trigger"
    class="theme-trigger"
    aria-label="Colour scheme"
    title="Colour scheme"
  >
    <Sun size={17} class="theme-sun" /><Moon size={17} class="theme-moon" />
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content class="theme-menu" align="end" sideOffset={8}>
      <DropdownMenu.RadioGroup bind:value aria-label="Colour scheme">
        {#each choices as choice (choice.value)}
          <DropdownMenu.RadioItem value={choice.value} class="theme-option">
            <choice.icon size={15} /><span>{choice.label}</span>
            {#if value === choice.value}<Check size={14} />{/if}
          </DropdownMenu.RadioItem>
        {/each}
      </DropdownMenu.RadioGroup>
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
