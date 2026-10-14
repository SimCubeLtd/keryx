import { test, expect } from '../../frontend/node_modules/@playwright/test/index.mjs';

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

test('selecting a second tag narrows results to drafts with both tags', async ({ page, request }) => {
  const { a, b, planning, review } = await seed(request);
  await page.goto('/');
  await page.locator('#tag-filter-summary').click();
  await page.locator(`[data-tag-choice="${planning.id}"]`).check();
  await expect(visibleRows(page)).toHaveCount(2);
  await page.locator(`[data-tag-choice="${review.id}"]`).check();
  await expect(visibleRows(page)).toHaveCount(1);
  await expect(visibleRows(page)).toHaveAttribute('data-draft-id', b);
  await expect(page.locator(`.draft-row[data-draft-id="${a}"]`)).toBeHidden();
  await page.locator(`[data-tag-choice="${review.id}"]`).uncheck();
  await expect(visibleRows(page)).toHaveCount(2);
});

test('tag filters combine with repository, search and availability; URL and snapshots retain state', async ({ page, request }) => {
  const { b, c, planning, review } = await seed(request);
  await controlledRefresh(page);
  await page.goto(`/?tag=${planning.id}&tag=${review.id}&draft=${b}&sort=tag`);
  await expect(visibleRows(page)).toHaveCount(1);
  await expect(page.locator('#tag-selections .tag-chip')).toHaveCount(3);
  await page.locator('#repo-filter').click();
  await page.getByRole('button', { name: 'test/two', exact: true }).click();
  await expect(visibleRows(page)).toHaveCount(1);
  await page.locator('#draft-search').fill('planning');
  await expect(visibleRows(page)).toHaveCount(1);
  await refresh(page);
  await expect(page.locator('#draft-search')).toHaveValue('planning');
  await expect(page.locator('#repo-filter')).toHaveText('test/two');
  await expect(page.locator('#detail-id')).toHaveText(b);
  await page.reload();
  await expect(visibleRows(page)).toHaveCount(1);
  await page.locator('#tag-filter-summary').click();
  await expect(page.locator('#tag-filter-options label').filter({ hasText: /planning/ })).toContainText('1');
  await page.locator('#tag-filter-done').click();
  await page.locator('#draft-search').fill('');
  await page.locator('#repo-filter').click();
  await page.getByRole('button', { name: /^All repositories/ }).click();
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

test('multi-select staging, new tags, Clear and Cancel never save before Apply', async ({ page, request }) => {
  const { a } = await seed(request);
  let writes = 0;
  page.on('request', request => { if (request.method() === 'POST' && request.url().endsWith('/tags')) writes++; });
  await page.goto(`/?draft=${a}`);
  await page.locator('#add-tag').click();
  await expect(page.locator('#tag-input')).toBeFocused();
  await expect(page.locator('#tag-suggestions [data-tag-name="planning"]')).toHaveAttribute('aria-disabled', 'true');
  await page.locator('#tag-suggestions [data-tag-name="review"]').click();
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeVisible();
  await page.locator('#tag-input').fill('Release Notes');
  await expect(page.locator('#tag-suggestions')).toContainText('Create "release notes"');
  await page.keyboard.press('Enter');
  await expect(page.locator('[aria-label="Pending tags"]')).toContainText('release notes');
  await expect(page.locator('#tag-apply')).toHaveText('Apply 2 tags');
  expect(writes).toBe(0);
  expect((await (await request.get('/api/dashboard/snapshot')).json()).tags.map(tag => tag.name)).not.toContain('release notes');
  await page.locator('#tag-clear').click();
  await expect(page.locator('#tag-apply')).toBeDisabled();
  await expect(page.locator('#tag-suggestions [data-tag-name="planning"]')).toHaveAttribute('aria-selected', 'true');
  await page.locator('#tag-input').fill('cancelled');
  await expect(page.locator('#tag-suggestions')).toContainText('Create');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeHidden();
  await expect(page.locator('#add-tag')).toBeFocused();
  expect(writes).toBe(0);
  await page.locator('#add-tag').click();
  await page.locator('#tag-suggestions [data-tag-name="review"]').click();
  await page.locator('#tag-input').fill('Release Notes');
  await expect(page.locator('#tag-suggestions')).toContainText('Create');
  await page.keyboard.press('Enter');
  await page.locator('#tag-apply').click();
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeHidden();
  expect(writes).toBe(1);
  await expect(page.locator('#detail-tags')).toContainText('planning');
  await expect(page.locator('#detail-tags')).toContainText('review');
  await expect(page.locator('#detail-tags')).toContainText('release notes');
});

test('IME and search debounce stage only the displayed choice, never an immediate write', async ({ page, request }) => {
  const { a } = await seed(request);
  await page.goto(`/?draft=${a}`);
  await page.clock.install();
  await page.clock.pauseAt(new Date(Date.now() + 1000));
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('created');
  await page.keyboard.press('Enter');
  await expect(page.locator('[aria-label="Pending tags"]')).toHaveCount(0);
  await page.clock.runFor(150);
  await expect(page.locator('#tag-suggestions')).toContainText('Create "created"');
  await page.locator('#tag-input').dispatchEvent('compositionstart');
  await page.locator('#tag-input').fill('review');
  await page.clock.runFor(300);
  await expect(page.locator('#tag-suggestions [role=option]')).toHaveCount(0);
  await page.locator('#tag-input').dispatchEvent('compositionend');
  await page.clock.runFor(150);
  await page.keyboard.press('Enter');
  await expect(page.locator('[aria-label="Pending tags"]')).toContainText('review');
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeVisible();
});

test('failed assignment retains staged choices across refresh and does not change target', async ({ page, request }) => {
  const { a } = await seed(request);
  await controlledRefresh(page);
  await page.goto(`/?draft=${a}`);
  await page.locator('#add-tag').click();
  await page.locator('#tag-input').fill('new-tag');
  await expect(page.locator('#tag-suggestions')).toContainText('Create');
  await page.keyboard.press('Enter');
  await refresh(page);
  await expect(page.locator('[aria-label="Pending tags"]')).toContainText('new-tag');
  await page.route('**/api/dashboard/drafts/*/tags', route => route.fulfill({ status: 500, json: { error: 'Save failed for test' } }));
  await page.locator('#tag-apply').click();
  await expect(page.locator('#tag-error')).toHaveText('Save failed for test');
  await expect(page.locator('[aria-label="Pending tags"]')).toContainText('new-tag');
  await expect(page.locator('#tag-input')).toBeFocused();
  await refresh(page);
  await expect(page.locator('#tag-target')).toHaveText('To Alpha');
  await page.unroute('**/api/dashboard/drafts/*/tags');
  await page.locator('#tag-apply').click();
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeHidden();
  await expect(page.locator('#detail-tags')).toContainText('new-tag');
  await page.locator('#add-tag').click();
  await request.delete(`/api/drafts/${a}?purge=true`);
  await refresh(page);
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeHidden();
  await expect(page.locator('#toast')).toContainText('no longer available');
});

test('assignment popover fits mobile in both themes and preserves pending choices during SSE', async ({ page, request }) => {
  const { a } = await seed(request);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?draft=${a}`);
  for (const theme of ['Dark', 'Light']) {
    await page.getByRole('button', { name: 'Colour scheme', exact: true }).click();
    await page.getByRole('menuitemradio', { name: theme, exact: true }).click();
    await page.locator('#add-tag').click();
    await page.locator('#tag-input').fill(`mobile-${theme.toLowerCase()}`);
    await expect(page.locator('#tag-suggestions')).toContainText('Create');
    await page.keyboard.press('Enter');
    await page.locator('#tag-input').fill('typing');
    await attach(request, a, `external-${theme.toLowerCase()}`);
    await expect(page.locator('#detail-tags')).toContainText(`external-${theme.toLowerCase()}`);
    await expect(page.locator('#tag-input')).toHaveValue('typing');
    await expect(page.locator('[aria-label="Pending tags"]')).toContainText(`mobile-${theme.toLowerCase()}`);
    await expect.poll(async () => {
      const bounds = await page.getByRole('dialog', { name: 'Add tags', exact: true }).boundingBox();
      return bounds.y + bounds.height;
    }).toBeLessThanOrEqual(844);
    const bounds = await page.getByRole('dialog', { name: 'Add tags', exact: true }).boundingBox();
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(844);
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
    await page.screenshot({ path: `/tmp/keryx/tags-popover-${theme.toLowerCase()}-mobile.png` });
    await page.locator('#tag-cancel').click();
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
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
