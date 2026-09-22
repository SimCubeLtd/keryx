import { test, expect } from '../../website/node_modules/@playwright/test/index.mjs';

async function upload(request, title, repo = 'one') {
  const response = await request.post('/api/uploads', { data: {
    html: `<!doctype html><html><head><title>${title}</title></head><body><h1>${title}</h1></body></html>`,
    metadata: { repoOrg: 'test', repoName: repo },
  } });
  expect(response.ok()).toBeTruthy();
  return (await response.json()).draftId;
}
async function attach(request, draft, name) {
  const response = await request.post(`/api/dashboard/drafts/${draft}/tags`, { data: { name } });
  expect(response.ok()).toBeTruthy();
  return (await response.json()).tags.find(tag => tag.name === name);
}
async function seed(request) {
  const a = await upload(request, 'Alpha');
  const b = await upload(request, 'Beta', 'two');
  const c = await upload(request, 'Gamma');
  const planning = await attach(request, a, 'planning');
  const review = await attach(request, b, 'review');
  await attach(request, b, 'planning');
  return { a, b, c, planning, review };
}
async function controlledRefresh(page) {
  await page.addInitScript(() => {
    window.EventSource = class { addEventListener(_, listener) { window.refreshTestDashboard = listener; } };
  });
}
async function refresh(page) {
  const response = page.waitForResponse('**/api/dashboard/snapshot*');
  await page.evaluate(() => window.refreshTestDashboard());
  await response;
}
const visibleRows = page => page.locator('.draft-row:visible');

test.beforeEach(async ({ request }) => {
  const response = await request.get('/api/drafts');
  const body = await response.json();
  for (const draft of body.drafts) await request.delete(`/api/drafts/${draft.draftId}?purge=true`);
});

test('ANY filters combine with repository, search and availability; URL and snapshots retain state', async ({ page, request }) => {
  const { a, b, c, planning, review } = await seed(request);
  await controlledRefresh(page);
  await page.goto(`/?tag=${planning.id}&tag=${review.id}&draft=${a}&sort=tag`);
  await expect(visibleRows(page)).toHaveCount(2);
  await expect(page.locator('#tag-selections .tag-chip')).toHaveCount(3);
  await page.locator('#repo-filter').selectOption('test/one');
  await expect(visibleRows(page)).toHaveCount(1);
  await page.locator('#draft-search').fill('planning');
  await expect(visibleRows(page)).toHaveCount(1);
  await refresh(page);
  await expect(page.locator('#draft-search')).toHaveValue('planning');
  await expect(page.locator('#repo-filter')).toHaveValue('test/one');
  await expect(page.locator('#detail-id')).toHaveText(a);
  await page.reload();
  await expect(visibleRows(page)).toHaveCount(1);
  await page.locator('#tag-filter-summary').click();
  await expect(page.locator('#tag-filter-options label').filter({ hasText: /planning/ })).toContainText('(1)');
  await page.locator('#tag-filter-done').click();
  await page.locator('#draft-search').fill('');
  await page.locator('#repo-filter').selectOption('');
  await page.locator('#tag-filter-summary').click();
  await page.locator('#tag-untagged').check();
  await expect(visibleRows(page)).toHaveCount(1);
  await expect(visibleRows(page)).toHaveAttribute('data-draft-id', c);
  await page.locator(`[data-tag-choice="${review.id}"]`).check();
  await expect(page.locator('#tag-untagged')).not.toBeChecked();
  await expect(visibleRows(page)).toHaveCount(1);
  await expect(visibleRows(page)).toHaveAttribute('data-draft-id', b);
  await page.keyboard.press('Escape');
  await expect(page.locator('#tag-filter')).not.toHaveAttribute('open');
  await page.goto('/?tag=missing');
  await expect(visibleRows(page)).toHaveCount(0);
  await refresh(page);
  await expect(visibleRows(page)).toHaveCount(0);
  await page.locator('#clear-empty-tags').click();
  await expect(visibleRows(page)).toHaveCount(3);
  await request.put(`/api/drafts/${b}/availability`, { data: { state: 'disabled' } });
  await page.goto(`/?tag=${review.id}&view=active`);
  await expect(visibleRows(page)).toHaveCount(0);
  await page.getByRole('tab', { name: /Disabled/ }).click();
  await expect(visibleRows(page)).toHaveCount(1);
});

test('modal debounce blocks stale selection, handles IME, ranks matches and cancels cleanly', async ({ page, request }) => {
  const { a, b } = await seed(request);
  await attach(request, b, 'reviewed');
  await attach(request, b, 'needs-review');
  await controlledRefresh(page);
  await page.goto(`/?draft=${a}`);
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  let writes = 0;
  page.on('request', r => { if (r.method() === 'POST' && r.url().endsWith('/tags')) writes++; });
  await page.getByRole('button', { name: 'Add tag', exact: true }).click();
  await expect(page.locator('#tag-input')).toBeFocused();
  await page.locator('#tag-input').fill('review');
  await page.keyboard.press('Enter');
  await page.clock.runFor(149);
  await expect(page.locator('#tag-suggestions [role=option]')).toHaveCount(0);
  expect(writes).toBe(0);
  await page.clock.runFor(1);
  await expect(page.locator('#tag-suggestions [role=option]').first()).toHaveText('review');
  await expect(page.locator('#tag-suggestions [role=option]').nth(1)).toHaveText('reviewed');
  await expect(page.locator('#tag-suggestions [role=option]').nth(2)).toHaveText('needs-review');
  await expect(page.locator('#tag-suggestions')).not.toContainText('Create');
  await page.locator('#tag-input').dispatchEvent('compositionstart');
  await page.locator('#tag-input').fill('planning');
  await page.clock.runFor(300);
  await expect(page.locator('#tag-suggestions [role=option]')).toHaveCount(0);
  await page.locator('#tag-input').dispatchEvent('compositionend');
  await page.clock.runFor(150);
  await expect(page.locator('#tag-suggestions [role=option]')).toHaveText('planning · Added');
  await page.keyboard.press('Enter');
  expect(writes).toBe(0);
  await page.locator('#tag-input').fill('cancelled');
  await page.keyboard.press('Escape');
  await page.clock.runFor(150);
  await expect(page.locator('#tag-dialog')).not.toBeVisible();
  await expect(page.locator('#add-tag')).toBeFocused();
  expect(writes).toBe(0);
});

test('save failures retain input; refresh preserves modal target; late responses cannot close a new modal', async ({ page, request }) => {
  const { a, b } = await seed(request);
  await controlledRefresh(page);
  await page.goto(`/?draft=${a}`);
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('new-tag');
  await expect(page.locator('#tag-suggestions')).toContainText('Create');
  await refresh(page);
  await expect(page.locator('#tag-input')).toHaveValue('new-tag');
  await expect(page.locator('#tag-dialog')).toBeVisible();
  await page.route('**/api/dashboard/drafts/*/tags', route => route.fulfill({ status: 500, json: { error: 'Save failed for test' } }));
  await page.keyboard.press('Enter');
  await expect(page.locator('#tag-error')).toHaveText('Save failed for test');
  await expect(page.locator('#tag-input')).toHaveValue('new-tag');
  await refresh(page);
  await expect(page.locator('#tag-error')).toHaveText('Save failed for test');
  await page.unroute('**/api/dashboard/drafts/*/tags');
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  let savedTarget;
  await page.route('**/api/dashboard/drafts/*/tags', async route => {
    savedTarget = route.request().url();
    await gate; await route.continue();
  });
  await page.keyboard.press('Enter');
  await expect(page.locator('#tag-status')).toHaveText('Saving…');
  await page.locator('#tag-cancel').click();
  await page.locator(`.draft-row[data-draft-id="${b}"]`).click();
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('another');
  release();
  await expect.poll(() => savedTarget).toContain(a);
  await expect(page.locator('#tag-input')).toHaveValue('another');
  await expect.poll(async () => (await (await request.get('/api/dashboard/snapshot')).json()).rows).toContain('new-tag');
  await refresh(page);
  await expect(page.locator('#tag-input')).toHaveValue('another');
  await expect(page.locator('#tag-target')).toHaveText('To Beta');
  await request.delete(`/api/drafts/${b}?purge=true`);
  await refresh(page);
  await expect(page.locator('#tag-dialog')).not.toBeVisible();
  await expect(page.locator('#toast')).toContainText('no longer available');
});

test('create, attach and remove work across live refreshes in mobile dark and light themes', async ({ page, request }) => {
  const { a, review } = await seed(request);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?draft=${a}`);
  await page.locator('#theme-select').selectOption('dark');
  await page.locator('#tag-filter-summary').click();
  const menu = await page.locator('.tag-filter-menu').boundingBox();
  expect(menu.x).toBeGreaterThanOrEqual(0);
  expect(menu.x + menu.width).toBeLessThanOrEqual(390);
  await page.locator('#tag-filter-search').fill('review');
  await page.locator(`[data-tag-choice="${review.id}"]`).check();
  await expect(visibleRows(page)).toHaveCount(1);
  await page.locator('#tag-filter [data-clear-tags]').click();
  await page.locator('#tag-filter-done').click();
  await page.locator(`.draft-row[data-draft-id="${a}"]`).click();
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('review');
  await expect(page.locator('#tag-suggestions [role=option]').first()).toHaveText('review');
  await page.screenshot({ path: '/tmp/keryx-tags-dark-mobile.png', fullPage: true });
  await page.keyboard.press('Enter');
  await expect(page.locator('#tag-dialog')).not.toBeVisible();
  await expect(page.locator('#detail-tags')).toContainText('review');
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('Release Notes');
  await expect(page.locator('#tag-suggestions')).toContainText('Create "release notes"');
  await page.keyboard.press('Enter');
  await expect(page.locator('#tag-dialog')).not.toBeVisible();
  await expect(page.locator('#detail-tags')).toContainText('release notes');
  await page.locator('#theme-select').selectOption('light');
  await page.screenshot({ path: '/tmp/keryx-tags-light-mobile.png', fullPage: true });
  await page.getByRole('button', { name: 'Remove tag review', exact: true }).click();
  await expect(page.locator('#detail-tags')).not.toContainText('review');
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('typing');
  await attach(request, a, 'external-change');
  await expect(page.locator('#detail-tags')).toContainText('external-change');
  await expect(page.locator('#tag-input')).toHaveValue('typing');
  await expect(page.locator('#tag-dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await page.locator(`[data-filter-tag="${review.id}"]`).first().click();
  await expect(visibleRows(page)).toHaveCount(1);
});

test('tag sorting, row chips, failed removal and keyboard focus survive refresh', async ({ page, request }) => {
  const { a, b, planning } = await seed(request);
  await controlledRefresh(page);
  await page.goto(`/?draft=${a}&sort=tag`);
  await expect(visibleRows(page).locator('.draft-title')).toHaveText(['Alpha', 'Beta', 'Gamma']);
  await page.route(`**/api/dashboard/drafts/${a}/tags/*`, route => route.fulfill({ status: 500, json: { error: 'Removal failed' } }));
  await page.getByRole('button', { name: 'Remove tag planning', exact: true }).click();
  await expect(page.locator('#toast')).toHaveText('Removal failed');
  await expect(page.locator('#detail-tags')).toContainText('planning');
  await page.locator('#add-tag').focus();
  await refresh(page);
  await expect(page.locator('#add-tag')).toBeFocused();
  await page.locator(`.draft-row[data-draft-id="${b}"] [data-filter-tag="${planning.id}"]`).focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#detail-id')).toHaveText(a);
  await expect(visibleRows(page)).toHaveCount(2);
  await expect(page.locator('#tag-selections')).toContainText('planning');
  await page.locator('#add-tag').click();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#tag-cancel')).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.locator('#tag-input')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#add-tag')).toBeFocused();
});
