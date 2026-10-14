import { test, expect } from '../../frontend/node_modules/@playwright/test/index.mjs';

async function upload(request, title, draftId, repo = 'bulk') {
  const response = await request.post('/api/uploads', { data: { draftId, html: `<!doctype html><title>${title}</title><p>${title}</p>`, metadata: { repoOrg: 'test', repoName: repo } } });
  expect(response.ok()).toBeTruthy();
  return response.json();
}
const detail = async (request, id) => (await (await request.get(`/api/drafts/${id}`)).json()).draft;
const select = (page, title) => page.getByRole('checkbox', { name: `Select ${title}`, exact: true });
async function start(page) { await page.locator('#select-drafts').click(); }
async function reviewed(page, action) {
  await page.locator(action === 'reset' ? '#bulk-reset' : '#bulk-delete').click();
  if (action === 'purge') await page.locator('#bulk-confirm-text').fill('DELETE');
  await expect(page.locator('#bulk-confirm')).toBeEnabled();
}
test.beforeEach(async ({ request }) => {
  for (const draft of (await (await request.get('/api/drafts')).json()).drafts) await request.delete(`/api/drafts/${draft.draftId}?purge=true`);
});

test('selection is separate from detail focus, scopes to filters and does not grow with SSE', async ({ page, request }) => {
  const a = await upload(request, 'Alpha');
  await upload(request, 'Beta', undefined, 'other');
  await page.goto(`/?draft=${a.draftId}`);
  const tags = await page.locator('#tag-filter-summary').boundingBox();
  await expect(page.locator('#select-drafts')).toHaveText('Bulk Actions');
  const entry = await page.locator('#select-drafts').boundingBox();
  expect(entry.x).toBeGreaterThanOrEqual(tags.x + tags.width);
  expect(Math.abs(entry.y - tags.y)).toBeLessThan(2);
  await start(page);
  await expect(page.locator('#select-drafts')).toHaveText('Cancel bulk actions');
  await expect(page.locator('.workspace')).toHaveClass(/bulk-mode/);
  await expect(page.locator('.draft-actions-trigger')).toHaveCount(0);
  await expect(page.locator('#bulk-reset')).toBeDisabled();
  await select(page, 'Alpha').check();
  await expect(page.locator('#detail-title')).toHaveText('Alpha');
  await expect(page.getByRole('checkbox', { name: 'Select all filtered drafts' })).toHaveJSProperty('indeterminate', true);
  await page.locator('#draft-sort').selectOption('title');
  await expect(select(page, 'Alpha')).toBeChecked();
  await upload(request, 'Added live');
  await expect(select(page, 'Added live')).toBeVisible();
  await expect(select(page, 'Added live')).not.toBeChecked();
  await expect(page.locator('.bulk-action-bar')).toContainText('1 selected');
  await page.locator('#draft-search').fill('Alpha');
  await expect(select(page, 'Alpha')).not.toBeChecked();
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await expect(page.locator('.bulk-action-bar')).toContainText('1 selected');
  await page.locator('#draft-search').fill('');
  await expect(page.locator('#bulk-reset')).toBeDisabled();
  await page.setViewportSize({ width: 768, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: 'dark' });
  const mobileTags = await page.locator('#tag-filter-summary').boundingBox();
  const mobileEntry = await page.locator('#select-drafts').boundingBox();
  expect(Math.abs(mobileEntry.y - mobileTags.y)).toBeLessThan(2);
  await select(page, 'Alpha').check();
  const table = await page.locator('.draft-table').boundingBox();
  const lastCell = await page.locator('.draft-row').first().locator('td:visible').last().boundingBox();
  expect(Math.abs(lastCell.x + lastCell.width - table.x - table.width)).toBeLessThan(2);
  await page.screenshot({ path: '/tmp/keryx/bulk-selection-mobile.png', fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator('#select-drafts').click();
  await expect(select(page, 'Alpha')).toHaveCount(0);
  await expect(page.locator('.workspace')).not.toHaveClass(/bulk-mode/);
  await expect(page.locator('.draft-actions-trigger')).toHaveCount(3);
});

test('bulk history reset keeps drafts, latest content and tags, skips v1, and leaves unselected history intact', async ({ page, request }) => {
  const a = await upload(request, 'Alpha old');
  await upload(request, 'Alpha latest', a.draftId);
  const b = await upload(request, 'Single v1');
  const untouched = await upload(request, 'Unselected old');
  await upload(request, 'Unselected latest', untouched.draftId);
  await request.post(`/api/dashboard/drafts/${a.draftId}/tags`, { data: { name: 'keep-me' } });
  await page.goto('/');
  await start(page);
  await select(page, 'Alpha latest').check();
  await select(page, 'Single v1').check();
  await reviewed(page, 'reset');
  await expect(page.locator('#bulk-dialog')).toContainText('Remove 1 older version');
  await page.screenshot({ path: '/tmp/keryx/bulk-reset-review-desktop.png' });
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('#bulk-dialog [data-bulk-status="reset"]')).toHaveCount(1);
  await expect(page.locator('#bulk-dialog [data-bulk-status="unchanged"]')).toHaveCount(1);
  await expect(page.locator('#select-drafts')).toHaveText('Bulk Actions');
  await expect(page.locator('.workspace')).not.toHaveClass(/bulk-mode/);
  await expect(page.locator('.draft-actions-trigger')).toHaveCount(3);
  await page.locator('#bulk-close').click();
  const history = await detail(request, a.draftId);
  expect(history.versions).toHaveLength(1);
  expect(history.versions[0].versionNumber).toBe(1);
  expect((await request.get(`/d/${a.draftId}`)).ok()).toBeTruthy();
  expect((await (await request.get(`/d/${a.draftId}/raw`)).text())).toContain('Alpha latest');
  expect((await detail(request, untouched.draftId)).versions).toHaveLength(2);
  expect((await detail(request, b.draftId)).versions).toHaveLength(1);
  await page.locator(`.draft-row[data-draft-id="${a.draftId}"]`).click();
  await expect(page.locator('#detail-tags')).toContainText('keep-me');
  expect((await upload(request, 'Alpha next', a.draftId)).versionNumber).toBe(2);
});

test('permanent deletion requires confirmation, deletes every selected version, and never touches unselected drafts', async ({ page, request }) => {
  const a = await upload(request, 'Delete old');
  await upload(request, 'Delete latest', a.draftId);
  const b = await upload(request, 'Keep this');
  await page.goto('/');
  await start(page);
  await select(page, 'Delete latest').check();
  await page.locator('#bulk-delete').click();
  await expect(page.locator('#bulk-confirm')).toBeDisabled();
  await page.locator('#bulk-confirm-text').fill('DELETE');
  await expect(page.locator('#bulk-confirm')).toBeEnabled();
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(page.locator('#select-drafts')).toHaveText('Cancel bulk actions');
  await expect(select(page, 'Delete latest')).toBeChecked();
  expect((await detail(request, a.draftId)).versions).toHaveLength(2);
  await reviewed(page, 'purge');
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('[data-bulk-status="deleted"]')).toHaveCount(1);
  expect((await request.get(`/api/drafts/${a.draftId}`)).status()).toBe(404);
  expect((await request.get(`/d/${a.draftId}/v/1/raw`)).status()).toBe(404);
  expect((await request.get(`/d/${a.draftId}/v/2/raw`)).status()).toBe(404);
  expect((await request.get(`/d/${b.draftId}`)).ok()).toBeTruthy();
});

test('a draft revised after review is not silently deleted, and stale reviews can be refreshed', async ({ page, request }) => {
  const a = await upload(request, 'Reviewed draft');
  await page.addInitScript(() => { window.EventSource = class { addEventListener(_, listener) { window.refreshBulk = listener; } }; });
  await page.goto('/');
  await start(page);
  await select(page, 'Reviewed draft').check();
  await reviewed(page, 'purge');
  await upload(request, 'New content', a.draftId);
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('[data-bulk-status="conflict"]')).toHaveCount(1);
  expect((await detail(request, a.draftId)).versions).toHaveLength(2);
  await page.locator('#bulk-close').click();
  await start(page);
  await select(page, 'New content').check();
  await reviewed(page, 'purge');
  await upload(request, 'Even newer', a.draftId);
  await page.evaluate(() => window.refreshBulk());
  await expect(page.locator('#bulk-dialog')).toContainText('A selected draft changed');
  await expect(page.locator('#bulk-confirm')).toBeDisabled();
  await page.getByRole('button', { name: 'Review again', exact: true }).click();
  await expect(page.locator('#bulk-confirm-text')).toHaveValue('');
  await page.locator('#bulk-confirm-text').fill('DELETE');
  await expect(page.locator('#bulk-confirm')).toBeEnabled();
});

test('bulk queue reports partial failures and stops after the in-flight request without starting more', async ({ page, request }) => {
  const a = await upload(request, 'Alpha');
  const b = await upload(request, 'Beta');
  const c = await upload(request, 'Gamma');
  await page.goto('/');
  await page.locator('#draft-sort').selectOption('title');
  await start(page);
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await page.route(`**/api/drafts/${a.draftId}?*`, route => route.fulfill({ status: 500, json: { error: 'Deliberate failure' } }));
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  let inFlight;
  const started = new Promise(resolve => { inFlight = resolve; });
  await page.route(`**/api/drafts/${b.draftId}?*`, async route => { inFlight(); await gate; await route.continue(); });
  await reviewed(page, 'purge');
  await page.locator('#bulk-confirm').click();
  await started;
  await expect(page.locator('#select-drafts')).toHaveText('Bulk Actions');
  await expect(page.locator('.workspace')).not.toHaveClass(/bulk-mode/);
  await expect(page.getByRole('checkbox', { name: 'Select all filtered drafts' })).toHaveCount(0);
  await expect(page.locator('.draft-actions-trigger')).toHaveCount(3);
  await expect(page.locator('#bulk-dialog')).toBeVisible();
  await page.getByRole('button', { name: 'Stop after current draft', exact: true }).click();
  release();
  await expect(page.locator('[data-bulk-status="failed"]')).toHaveCount(1);
  await expect(page.locator('[data-bulk-status="deleted"]')).toHaveCount(1);
  await expect(page.locator('[data-bulk-status="unattempted"]')).toHaveCount(1);
  expect((await request.get(`/d/${a.draftId}`)).ok()).toBeTruthy();
  expect((await request.get(`/d/${b.draftId}`)).status()).toBe(404);
  expect((await request.get(`/d/${c.draftId}`)).ok()).toBeTruthy();
});

test('auth failures and lost responses stop the queue; cleanup warnings stay visible', async ({ page, request }) => {
  await upload(request, 'Alpha');
  const b = await upload(request, 'Beta');
  await page.goto('/');
  await page.locator('#draft-sort').selectOption('title');
  await start(page);
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await page.route('**/api/drafts/*?*', route => route.fulfill({ status: 401, json: { error: 'Unauthorized' } }));
  await reviewed(page, 'purge');
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('[data-bulk-status="unattempted"]')).toHaveCount(1);
  await page.locator('#bulk-close').click();
  await page.unroute('**/api/drafts/*?*');
  await start(page);
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await page.route('**/api/drafts/*?*', route => route.abort());
  await reviewed(page, 'purge');
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('[data-bulk-status="unknown"]')).toHaveCount(1);
  await expect(page.locator('[data-bulk-status="unattempted"]')).toHaveCount(1);
  await page.locator('#bulk-close').click();
  await page.unroute('**/api/drafts/*?*');
  await start(page);
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await page.route('**/api/drafts/*?*', route => route.fulfill({ json: { ok: true, purged: true, cleanupComplete: false } }));
  await reviewed(page, 'purge');
  await page.locator('#bulk-confirm').click();
  await expect(page.locator('#bulk-dialog')).toContainText('storage cleanup pending');
  expect((await request.get(`/d/${b.draftId}`)).ok()).toBeTruthy();
});

for (const theme of ['dark', 'light']) test(`confirmation field and destructive button states are visible in ${theme} mode`, async ({ page, request }) => {
  await upload(request, 'Appearance check');
  await page.emulateMedia({ colorScheme: theme });
  if (theme === 'dark') await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await start(page);
  await expect(page.locator('#select-drafts')).toHaveText('Cancel bulk actions');
  await select(page, 'Appearance check').check();
  const toolbarAppearance = await page.locator('#bulk-delete').evaluate(button => {
    const style = getComputedStyle(button);
    return { background: style.backgroundColor, text: style.color, disabled: button.disabled };
  });
  expect(toolbarAppearance.disabled).toBe(false);
  expect(toolbarAppearance.text).not.toBe('rgb(255, 255, 255)');
  await page.locator('#bulk-delete').click();
  const input = page.locator('#bulk-confirm-text');
  await expect(input).toBeEnabled();
  const fieldAppearance = await input.evaluate(element => {
    const style = getComputedStyle(element);
    return { border: parseFloat(style.borderTopWidth), background: style.backgroundColor, height: element.getBoundingClientRect().height };
  });
  expect(fieldAppearance.border).toBeGreaterThanOrEqual(1);
  expect(fieldAppearance.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(fieldAppearance.height).toBeGreaterThanOrEqual(38);
  await expect(page.locator('#bulk-confirm')).toBeDisabled();
  const disabledAppearance = await page.locator('#bulk-confirm').evaluate(button => {
    const style = getComputedStyle(button);
    return { background: style.backgroundColor, text: style.color, opacity: style.opacity };
  });
  expect(disabledAppearance.text).not.toBe('rgb(255, 255, 255)');
  expect(disabledAppearance.opacity).toBe('1');
  await page.screenshot({ path: `/tmp/keryx/bulk-delete-disabled-${theme}.png` });
  await input.fill('DELETE');
  await expect(page.locator('#bulk-confirm')).toBeEnabled();
  const enabledAppearance = await page.locator('#bulk-confirm').evaluate(button => {
    const style = getComputedStyle(button);
    const parse = value => value.match(/[\d.]+/g).slice(0, 3).map(Number).map(channel => {
      const normalized = channel / 255;
      return normalized <= 0.04045 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
    });
    const luminance = value => { const [r, g, b] = parse(value); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
    const light = Math.max(luminance(style.color), luminance(style.backgroundColor));
    const dark = Math.min(luminance(style.color), luminance(style.backgroundColor));
    return { background: style.backgroundColor, text: style.color, contrast: (light + 0.05) / (dark + 0.05) };
  });
  expect(enabledAppearance.background).not.toBe(disabledAppearance.background);
  expect(enabledAppearance.background).not.toBe(toolbarAppearance.background);
  expect(enabledAppearance.contrast).toBeGreaterThanOrEqual(4.5);
  await page.screenshot({ path: `/tmp/keryx/bulk-delete-enabled-${theme}.png` });
});


test('bulk tags stage multiple names, preserve existing tags and selection on cancel, and save against frozen targets', async ({ page, request }) => {
  const a = await upload(request, 'Tag Alpha');
  const b = await upload(request, 'Tag Beta');
  const c = await upload(request, 'Untouched');
  await request.post(`/api/dashboard/drafts/${a.draftId}/tags`, { data: { names: ['shared', 'partial'] } });
  await request.post(`/api/dashboard/drafts/${b.draftId}/tags`, { data: { names: ['shared'] } });
  await page.goto('/');
  await start(page);
  await select(page, 'Tag Alpha').check();
  await select(page, 'Tag Beta').check();
  await page.locator('#bulk-tags').click();
  await expect(page.locator('[data-tag-name="shared"]')).toBeDisabled();
  await expect(page.locator('[data-tag-name="partial"]')).toContainText('On 1 of 2');
  await page.locator('[data-tag-name="partial"]').click();
  await page.locator('#tag-cancel').click();
  await expect(select(page, 'Tag Alpha')).toBeChecked();
  await page.locator('#bulk-tags').click();
  await page.locator('[data-tag-name="partial"]').click();
  await page.locator('#tag-input').fill('new bulk');
  await page.locator('[data-tag-name="new bulk"]').click();
  const bodies = [];
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  await page.route('**/api/dashboard/drafts/*/tags', async route => {
    bodies.push(route.request().postDataJSON());
    if (bodies.length === 1) await gate;
    await route.continue();
  });
  await page.locator('#tag-apply').click();
  await expect(page.locator('#select-drafts')).toHaveText('Bulk Actions');
  await expect(page.getByRole('dialog', { name: 'Add tags', exact: true })).toBeVisible();
  await expect(page.locator('[data-tag-result="saving"]')).toHaveCount(1);
  release();
  await expect(page.locator('[data-tag-result="assigned"]')).toHaveCount(2);
  expect(bodies).toEqual([{ names: ['partial', 'new bulk'] }, { names: ['partial', 'new bulk'] }]);
  await page.locator('#tag-done').click();
  const snapshot = await (await request.get('/api/dashboard/snapshot')).json();
  for (const id of [a.draftId, b.draftId]) expect(snapshot.drafts.find(row => row.summary.draftId === id).tags.map(tag => tag.name).sort()).toEqual(['new bulk', 'partial', 'shared']);
  expect(snapshot.drafts.find(row => row.summary.draftId === c.draftId).tags).toHaveLength(0);
});

test('bulk tag limit failure leaves that draft unchanged and allows other drafts to succeed', async ({ page, request }) => {
  const a = await upload(request, 'At limit');
  const b = await upload(request, 'Has room');
  await request.post(`/api/dashboard/drafts/${a.draftId}/tags`, { data: { names: Array.from({ length: 19 }, (_, i) => `tag-${i}`) } });
  await page.goto('/');
  await start(page);
  await page.getByRole('checkbox', { name: 'Select all filtered drafts' }).check();
  await page.locator('#bulk-tags').click();
  for (const name of ['new one', 'new two']) {
    await page.locator('#tag-input').fill(name);
    await page.locator(`[data-tag-name="${name}"]`).click();
  }
  await page.locator('#tag-apply').click();
  await expect(page.locator('[data-tag-result="failed"]')).toHaveCount(1);
  await expect(page.locator('[data-tag-result="assigned"]')).toHaveCount(1);
  const snapshot = await (await request.get('/api/dashboard/snapshot')).json();
  expect(snapshot.drafts.find(row => row.summary.draftId === a.draftId).tags).toHaveLength(19);
  expect(snapshot.drafts.find(row => row.summary.draftId === b.draftId).tags.map(tag => tag.name).sort()).toEqual(['new one', 'new two']);
});
