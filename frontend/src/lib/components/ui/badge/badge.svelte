<script lang="ts">
  import type { HTMLAttributes } from 'svelte/elements'
  import { cn } from '$lib/utils'
  let { class: className, children, ...rest }: HTMLAttributes<HTMLSpanElement> = $props()
</script>

<span class={cn('status', className)} {...rest}>{@render children?.()}</span>
