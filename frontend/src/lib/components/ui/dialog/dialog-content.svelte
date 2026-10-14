<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import type { WithoutChild } from '$lib/utils'
  import { cn } from '$lib/utils'
  let {
    ref = $bindable(null),
    class: className,
    children,
    ...rest
  }: WithoutChild<DialogPrimitive.ContentProps> = $props()
</script>

<DialogPrimitive.Portal>
  <DialogPrimitive.Overlay class="dialog-overlay" />
  <DialogPrimitive.Content bind:ref class={cn('shadcn-dialog', className)} {...rest}>
    <div class="dialog-body">{@render children?.()}</div>
  </DialogPrimitive.Content>
</DialogPrimitive.Portal>
