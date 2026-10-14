<script lang="ts">
  import { onMount, tick } from 'svelte'
  import type {
    NotificationKind,
    PushSubscriptionInput,
    PushSubscriptionSummary,
  } from '$lib/generated/bindings'
  import { errorMessage, request } from '$lib/api'
  import { Button } from './ui/button'
  let {
    management,
    toast,
  }: { management: boolean; toast: (message: string, error?: boolean) => void } = $props()
  interface InstallPrompt extends Event {
    prompt(): Promise<void>
    userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>
  }
  let install = $state<InstallPrompt | null>(null)
  let supported = $state(false),
    busy = $state(false),
    subscription = $state<PushSubscription | null>(null)
  let pendingSubscription = $state<PushSubscription | null>(null)
  let restorePending = true
  let registrationError = $state('')
  let savedEvents: NotificationKind[] = []
  let permission = $state<NotificationPermission>('default')
  let events = $state<NotificationKind[]>(['published', 'revised', 'woke', 'enabled', 'disabled'])
  const labels: Record<NotificationKind, string> = {
    published: 'Plan published',
    revised: 'Plan revised',
    woke: 'Plan woke',
    enabled: 'Plan enabled',
    disabled: 'Plan disabled',
  }
  function bytes(value: string) {
    const raw = atob(
      (value + '='.repeat((4 - (value.length % 4)) % 4)).replace(/-/g, '+').replace(/_/g, '/'),
    )
    return Uint8Array.from(raw, (char) => char.charCodeAt(0))
  }
  async function save(current: PushSubscription, restore = false) {
    const payload = current.toJSON()
    if (!payload.keys?.p256dh || !payload.keys.auth)
      throw new Error('Push subscription keys are unavailable.')
    const body: PushSubscriptionInput = {
      endpoint: current.endpoint,
      keys: { p256dh: payload.keys.p256dh, auth: payload.keys.auth },
      events: restore ? undefined : events,
    }
    const response = await request<{ subscription: PushSubscriptionSummary }>(
      '/api/push/subscriptions',
      { method: 'PUT', body: JSON.stringify(body) },
    )
    events = response.subscription.events
    savedEvents = [...events]
    subscription = current
    pendingSubscription = null
    registrationError = ''
  }
  onMount(() => {
    let active = true
    if (window.isSecureContext && 'serviceWorker' in navigator) {
      void navigator.serviceWorker
        .register('/sw.js')
        .then(async (registration) => {
          if ('PushManager' in window && 'Notification' in window) {
            const current = await registration.pushManager.getSubscription()
            if (active) {
              supported = true
              permission = Notification.permission
              if (current) {
                pendingSubscription = current
                busy = true
                try {
                  await save(current, true)
                } catch (cause) {
                  if (active) registrationError = errorMessage(cause)
                } finally {
                  if (active) busy = false
                }
              }
            }
          }
        })
        .catch((error) => console.warn('Keryx service worker unavailable:', error))
    }
    const beforeInstall = (event: Event) => {
      event.preventDefault()
      install = event as InstallPrompt
    }
    const installed = () => {
      install = null
      toast('Keryx installed.')
    }
    window.addEventListener('beforeinstallprompt', beforeInstall)
    window.addEventListener('appinstalled', installed)
    return () => {
      active = false
      window.removeEventListener('beforeinstallprompt', beforeInstall)
      window.removeEventListener('appinstalled', installed)
    }
  })
  async function installApp() {
    const prompt = install
    if (!prompt) return
    try {
      await prompt.prompt()
      await prompt.userChoice
    } catch (cause) {
      toast(errorMessage(cause), true)
    } finally {
      install = null
    }
  }
  async function enable() {
    busy = true
    try {
      if (pendingSubscription) {
        await save(pendingSubscription, restorePending)
        toast('Notifications enabled on this device.')
        return
      }
      permission = await Notification.requestPermission()
      if (permission !== 'granted') return
      const { publicKey } = await request<{ publicKey: string }>('/api/push/vapid')
      const registration = await navigator.serviceWorker.ready
      const current = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: bytes(publicKey),
      })
      pendingSubscription = current
      restorePending = false
      await save(current)
      toast('Notifications enabled on this device.')
    } catch (cause) {
      if (pendingSubscription) registrationError = errorMessage(cause)
      toast(errorMessage(cause), true)
    } finally {
      busy = false
    }
  }
  async function disable() {
    busy = true
    try {
      if (subscription) {
        await request('/api/push/subscriptions', {
          method: 'DELETE',
          body: JSON.stringify({ endpoint: subscription.endpoint }),
        })
        await subscription.unsubscribe()
      }
      subscription = null
      toast('Notifications turned off on this device.')
    } catch (cause) {
      toast(errorMessage(cause), true)
    } finally {
      busy = false
    }
  }
  async function preferences() {
    if (!subscription) return
    busy = true
    try {
      await save(subscription)
    } catch (cause) {
      events = [...savedEvents]
      toast(errorMessage(cause), true)
    } finally {
      busy = false
    }
  }
</script>

{#if supported && management}
  <details class="notify-menu" id="notify-menu">
    <summary id="notify-summary" class:on={!!subscription}
      >Notifications {subscription ? 'on' : 'off'}</summary
    >
    <div class="menu notify-panel">
      <div class="notify-state" id="notify-state">
        {registrationError
          ? `Registration failed: ${registrationError}. Retry to enable notifications.`
          : subscription
            ? 'This device receives the selected draft events.'
            : permission === 'denied'
              ? 'Notifications are blocked in this browser. Change its site permissions to enable them.'
              : 'Enable notifications for this device.'}
      </div>
      <div class="menu-group">
        {#each Object.entries(labels) as [kind, label] (kind)}<label class="notify-option"
            ><input
              type="checkbox"
              value={kind}
              bind:group={events}
              disabled={busy || !!pendingSubscription}
              onchange={async () => {
                await tick()
                await preferences()
              }}
              data-event={kind}
            />{label}</label
          >{/each}
      </div>
      <div class="notify-actions">
        <Button
          id="notify-enable"
          disabled={busy || permission === 'denied'}
          hidden={!!subscription}
          onclick={enable}
          >{pendingSubscription ? 'Retry registration' : 'Enable on this device'}</Button
        ><Button
          variant="outline"
          id="notify-disable"
          disabled={busy}
          hidden={!subscription}
          onclick={disable}>Turn off</Button
        >
      </div>
    </div>
  </details>
{/if}
<Button variant="outline" class="install" id="install-app" hidden={!install} onclick={installApp}
  >Install Keryx</Button
>
