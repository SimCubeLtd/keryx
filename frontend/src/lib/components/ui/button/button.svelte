<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'
  export const buttonVariants = tv({
    base: 'button',
    variants: {
      variant: {
        default: 'primary',
        outline: '',
        destructive: 'danger',
        'destructive-outline': 'danger-outline',
        secondary: '',
        ghost: 'border-transparent',
        link: 'underline',
      },
      size: { default: '', sm: 'text-xs', icon: 'icon-button' },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  })
  export type ButtonVariant = VariantProps<typeof buttonVariants>['variant']
</script>

<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import type { WithElementRef } from '$lib/utils'
  import { cn } from '$lib/utils'
  let {
    class: className,
    variant = 'default',
    size = 'default',
    ref = $bindable(null),
    type = 'button',
    children,
    ...rest
  }: WithElementRef<HTMLButtonAttributes> & VariantProps<typeof buttonVariants> = $props()
</script>

<button bind:this={ref} {type} class={cn(buttonVariants({ variant, size }), className)} {...rest}
  >{@render children?.()}</button
>
