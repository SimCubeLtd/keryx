import { test, expect } from '../../frontend/node_modules/@playwright/test/index.mjs';

async function upload(request, title, metadata) {
  const response = await request.post('/api/uploads', { data: { html: `<!doctype html><title>${title}</title><p>Isolated test</p>`, metadata } });
  expect(response.ok()).toBeTruthy();
  return (await response.json()).draftId;
}

test.beforeEach(async ({ request }) => {
  for (const draft of (await (await request.get('/api/drafts')).json()).drafts) {
    await request.delete(`/api/drafts/${draft.draftId}?purge=true`);
  }
});

test('the standalone executable serves the app assets and never turns missing routes into dashboard HTML', async ({ page, request }) => {
  const id = await upload(request, 'Embedded application');
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(`/?draft=${id}`);
  await expect(page.locator('#detail-title')).toHaveText('Embedded application');
  const source = await (await request.get('/')).text();
  const script = source.match(/src="(\/assets\/[^\"]+\.js)"/);
  expect(script).not.toBeNull();
  const asset = await request.get(script[1]);
  expect(asset.ok()).toBeTruthy();
  expect(asset.headers()['content-type']).toContain('javascript');
  for (const path of ['/assets/missing.js', '/api/missing', '/d/missing']) {
    expect((await request.get(path)).status()).toBe(404);
  }
  expect(errors).toEqual([]);
});

test('availability controls and client-side snooze expiry work with live snapshots', async ({ page, request }) => {
  const id = await upload(request, 'Availability example');
  await page.goto(`/?draft=${id}`);
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
  await page.locator('#action-disable').click();
  await expect(page.locator('.draft-row:visible')).toHaveCount(0);
  expect((await request.get(`/d/${id}`)).status()).toBe(404);
  await page.getByRole('tab', { name: /Disabled/ }).click();
  await page.locator('#action-enable').click();
  await page.getByRole('tab', { name: /Active/ }).click();
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
  expect((await request.get(`/d/${id}`)).ok()).toBeTruthy();
  await page.locator('#action-snooze').click();
  await page.locator('[data-preset="1h"]').click();
  await page.locator('#snooze-confirm').click();
  await page.getByRole('tab', { name: /Snoozed/ }).click();
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
  expect((await request.get(`/d/${id}`)).ok()).toBeTruthy();
  await page.locator('#action-unsnooze').click();
  await page.getByRole('tab', { name: /Active/ }).click();
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
  await page.clock.install();
  await request.put(`/api/drafts/${id}/availability`, { data: { state: 'snoozed', until: new Date(Date.now() + 60000).toISOString() } });
  await page.getByRole('tab', { name: /Snoozed/ }).click();
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
  await page.clock.runFor(61000);
  await expect(page.locator('.draft-row:visible')).toHaveCount(0);
  await page.getByRole('tab', { name: /Active/ }).click();
  await expect(page.locator('#detail-title')).toHaveText('Availability example');
});

test('the existing PWA worker registers at root scope and controls the dashboard', async ({ page, request }) => {
  await page.goto('/');
  await page.waitForFunction(async () => {
    const registration = await navigator.serviceWorker.getRegistration('/');
    return registration?.active?.scriptURL.endsWith('/sw.js') && navigator.serviceWorker.controller !== null;
  });
  const manifest = await (await request.get('/manifest.webmanifest')).json();
  expect(manifest.id).toBe('/');
  expect(manifest.start_url).toBe('/');
  await page.reload();
  await expect(page.locator('h1')).toHaveText('Drafts');
});

test('push controls send current event preferences and unsubscribe without contacting a push service', async ({ page }) => {
  await page.addInitScript(() => {
    let current = null;
    const subscription = {
      endpoint: 'https://push.example.test/subscription',
      toJSON: () => ({ keys: { p256dh: 'test-key', auth: 'test-auth' } }),
      unsubscribe: async () => { current = null; return true; },
    };
    const registration = { pushManager: {
      getSubscription: async () => current,
      subscribe: async () => { current = subscription; return subscription; },
    } };
    Object.defineProperty(navigator.serviceWorker, 'register', { value: async () => registration });
    Object.defineProperty(navigator.serviceWorker, 'ready', { get: () => Promise.resolve(registration) });
    Object.defineProperty(window, 'Notification', { value: class {
      static permission = 'default';
      static async requestPermission() { this.permission = 'granted'; return 'granted'; }
    } });
    if (!('PushManager' in window)) Object.defineProperty(window, 'PushManager', { value: class {} });
  });
  const writes = [];
  await page.route('**/api/push/vapid', route => route.fulfill({ json: { publicKey: 'AQID' } }));
  await page.route('**/api/push/subscriptions', route => {
    writes.push({ method: route.request().method(), body: route.request().postDataJSON() });
    return route.fulfill({ json: { ok: true } });
  });
  await page.goto('/');
  await page.locator('#notify-summary').click();
  await page.locator('#notify-enable').click();
  await expect(page.locator('#notify-summary')).toHaveText('Notifications on');
  expect(writes[0].body.events).toEqual(['published', 'revised', 'woke', 'enabled', 'disabled']);
  await page.locator('[data-event="revised"]').uncheck();
  await expect.poll(() => writes.length).toBe(2);
  expect(writes[1].body.events).toEqual(['published', 'woke', 'enabled', 'disabled']);
  await page.locator('#notify-disable').click();
  await expect(page.locator('#notify-summary')).toHaveText('Notifications off');
  expect(writes[2]).toEqual({ method: 'DELETE', body: { endpoint: 'https://push.example.test/subscription' } });
});

test('colour scheme menu persists explicit choices and follows the system when selected', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  const trigger = page.getByRole('button', { name: 'Colour scheme', exact: true });
  await trigger.click();
  await page.getByRole('menuitemradio', { name: 'Dark', exact: true }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await trigger.click();
  await expect(page.getByRole('menuitemradio', { name: 'Dark', exact: true })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('menuitemradio', { name: 'Light', exact: true }).click();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await trigger.click();
  await page.getByRole('menuitemradio', { name: 'System', exact: true }).click();
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveClass(/dark/);
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await trigger.click();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toBeHidden();
  await expect(trigger).toBeFocused();
});

test('grouped repository picker searches owners, selects exactly one repository and clears it', async ({ page, request }) => {
  await upload(request, 'First owner', { repoOrg: 'alpha', repoName: 'shared' });
  await upload(request, 'Second owner', { repoOrg: 'beta', repoName: 'shared' });
  await upload(request, 'Other repository', { repoOrg: 'alpha', repoName: 'other' });
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  const trigger = page.locator('#repo-filter');
  await trigger.click();
  const menu = page.getByRole('dialog', { name: 'Filter by repository', exact: true });
  const search = page.getByRole('searchbox', { name: 'Find a repository', exact: true });
  await expect(search).toBeFocused();
  await expect(menu.getByRole('heading')).toHaveText(['alpha', 'beta']);
  const bounds = await menu.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: '/tmp/keryx-repositories-dark-mobile.png' });
  await search.fill('BETA');
  await expect(menu.getByRole('heading')).toHaveText(['beta']);
  await page.getByRole('button', { name: 'beta/shared', exact: true }).click();
  await expect(menu).toBeHidden();
  await expect(page.locator('.draft-row:visible')).toHaveCount(1);
  await expect(page.locator('#detail-title')).toHaveText('Second owner');
  await trigger.click();
  await expect(search).toHaveValue('');
  await page.getByRole('button', { name: 'alpha/shared', exact: true }).click();
  await expect(page.locator('.draft-row:visible')).toHaveCount(1);
  await expect(page.locator('#detail-title')).toHaveText('First owner');
  await page.reload();
  await expect(trigger).toHaveText('alpha/shared');
  await trigger.click();
  await expect(menu.getByRole('button', { pressed: true })).toHaveCount(1);
  await search.fill('no-such-repository');
  await expect(menu).toContainText('No matching repositories');
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await trigger.click();
  await menu.getByRole('button', { name: /^All repositories/ }).click();
  await expect(page.locator('.draft-row:visible')).toHaveCount(3);
  await expect(trigger).toHaveText('All repositories');
  expect(new URL(page.url()).searchParams.has('repo')).toBe(false);
});

test('row actions manage the clicked draft, preserve confirmations and follow availability', async ({ page, request }) => {
  const selected = await upload(request, 'Selected draft');
  const target = await upload(request, 'Menu target');
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto(`/?draft=${selected}`);
  await expect(page.locator('#detail-title')).toHaveText('Selected draft');
  const trigger = page.getByRole('button', { name: 'Actions for Menu target', exact: true });
  const menu = page.getByRole('menu', { name: 'Actions for Menu target', exact: true });
  await trigger.click();
  await expect(menu).toContainText('Open & export');
  await expect(menu).toContainText('History & removal');
  await page.screenshot({ path: '/tmp/keryx/context-menu-dark-desktop.png' });
  await menu.locator('[data-action="tags"]').click();
  await expect(page.locator('#tag-target')).toHaveText('To Menu target');
  await expect(page.locator('#tag-input')).toBeFocused();
  await page.keyboard.press('Escape');
  await trigger.click();
  await menu.locator('[data-action="snooze"]').click();
  await expect(page.locator('#snooze-draft-title')).toHaveText('Menu target');
  await page.locator('[data-preset="1h"]').click();
  await page.locator('#snooze-confirm').click();
  await expect(page.locator('#snooze-dialog')).toBeHidden();
  await expect(page.locator('#detail-title')).toHaveText('Selected draft');
  await page.getByRole('tab', { name: /Snoozed/ }).click();
  await trigger.click();
  await expect(menu.locator('[data-action="snooze"]')).toHaveCount(0);
  await menu.locator('[data-action="unsnooze"]').click();
  await expect(page.locator('.draft-row:visible')).toHaveCount(0);
  await page.getByRole('tab', { name: /Active/ }).click();
  await page.route(`**/api/drafts/${target}/availability`, route => route.fulfill({ status: 500, json: { error: 'Availability failed for test' } }));
  await trigger.click();
  await menu.locator('[data-action="disable"]').click();
  await expect(page.locator('#toast')).toContainText('Availability failed for test');
  await expect(page.locator(`.draft-row[data-draft-id="${target}"]`)).toBeVisible();
  await page.unroute(`**/api/drafts/${target}/availability`);
  await trigger.click();
  await menu.locator('[data-action="disable"]').click();
  await expect(page.locator(`.draft-row[data-draft-id="${target}"]`)).toBeHidden();
  expect((await request.get(`/d/${target}`)).status()).toBe(404);
  expect((await request.get(`/d/${selected}`)).ok()).toBeTruthy();
  await page.getByRole('tab', { name: /Disabled/ }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: 'light' });
  await trigger.click();
  await expect(menu.getByRole('menuitem', { name: 'Open document', exact: true })).toHaveAttribute('aria-disabled', 'true');
  await expect(menu.locator('[data-action="snooze"]')).toHaveCount(0);
  await expect(menu.locator('[data-action="disable"]')).toHaveCount(0);
  const bounds = await menu.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: '/tmp/keryx/context-menu-light-mobile.png' });
  await menu.locator('[data-action="enable"]').click();
  await page.getByRole('tab', { name: /Active/ }).click();
  await trigger.click();
  await menu.locator('[data-action="prune"]').click();
  await expect(page.locator('#dialog-draft-title')).toHaveText('Menu target');
  await page.locator('#cancel-action').click();
  await trigger.click();
  await menu.locator('[data-action="purge"]').click();
  await expect(page.locator('#dialog-title')).toHaveText('Permanently delete this draft?');
  await page.locator('#cancel-action').click();
  expect((await request.get(`/d/${target}`)).ok()).toBeTruthy();
  await trigger.focus();
  await page.keyboard.press('ArrowDown');
  await expect(menu).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
});
