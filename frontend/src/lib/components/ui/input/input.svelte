<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements'
  import type { WithElementRef } from '$lib/utils'
  import { cn } from '$lib/utils'
  let {
    ref = $bindable(null),
    value = $bindable(),
    class: className,
    ...rest
  }: WithElementRef<HTMLInputAttributes> = $props()
</script>

<input bind:this={ref} bind:value class={cn('input', className)} {...rest} />
