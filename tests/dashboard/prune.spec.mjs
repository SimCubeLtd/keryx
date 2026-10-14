import { test, expect } from '../../frontend/node_modules/@playwright/test/index.mjs';

async function upload(request, draftId, title = 'Reset example') {
  const response = await request.post('/api/uploads', { data: {
    draftId,
    html: `<!doctype html><html><head><title>${title}</title></head><body><h1>${title}</h1></body></html>`,
    metadata: { repoOrg: 'test', repoName: 'prune' },
  } });
  expect(response.ok()).toBeTruthy();
  return response.json();
}

const draftRow = (page, id) => page.locator(`.draft-row[data-draft-id="${id}"]`);
const dialog = page => page.locator('#reset-history-dialog');
const confirm = page => page.locator('#reset-history-confirm');

async function openMenuAction(page, id) {
  await draftRow(page, id).getByRole('button', { name: /^Actions for/ }).click();
  await page.getByRole('menu').locator('[data-action="reset-history"]').click();
  await expect(confirm(page)).toBeEnabled();
}

test.beforeEach(async ({ request }) => {
  const response = await request.get('/api/drafts');
  for (const draft of (await response.json()).drafts) {
    await request.delete(`/api/drafts/${draft.draftId}?purge=true`);
  }
});

test('both entry points share confirmation; reset refreshes both dashboards and uploads resume at v2', async ({ page, request, context }) => {
  const first = await upload(request);
  const id = first.draftId;
  await upload(request, id, 'Latest content');
  const other = await upload(request, undefined, 'Other draft');
  await page.goto(`/?draft=${id}&repo=test%2Fprune&search=Latest`);
  await expect(page.locator('#version-history .history-item')).toHaveCount(2);
  const observer = await context.newPage();
  await observer.goto(`/?draft=${id}`);
  await expect(observer.locator('#version-history .history-item')).toHaveCount(2);

  await page.locator('#reset-history').click();
  await expect(confirm(page)).toBeEnabled();
  await expect(page.locator('#reset-history-description')).toHaveText('Permanently delete 1 older version and renumber v2 as v1. The next upload will be v2.');
  await expect(page.locator('#reset-history-warning')).toContainText('Existing versioned links may stop working or show different content.');
  await page.locator('#reset-history-cancel').click();
  await expect(dialog(page)).not.toBeVisible();
  await expect(page.locator('#version-history .history-item')).toHaveCount(2);

  await openMenuAction(page, id);
  await expect(page.locator('#reset-history-description')).toHaveText('Permanently delete 1 older version and renumber v2 as v1. The next upload will be v2.');
  await confirm(page).click();
  await expect(dialog(page)).not.toBeVisible();
  for (const dashboard of [page, observer]) {
    await expect(dashboard.locator('#version-history .history-item')).toHaveCount(1);
    await expect(dashboard.locator('#version-history .vbadge')).toHaveText('v1');
    await expect(draftRow(dashboard, id)).toHaveAttribute('data-version-count', '1');
    await expect(dashboard.locator('#reset-history')).toBeHidden();
    await expect(draftRow(dashboard, id).locator('[data-action="reset-history"]')).toBeHidden();
  }
  await expect(page.locator('#detail-version')).toHaveText('v1 · 1 version');
  await expect(page.locator('#detail-title')).toHaveText('Latest content');
  await expect(page.locator('#draft-search')).toHaveValue('Latest');
  await expect(page.locator('#repo-filter')).toHaveText('test/prune');
  expect((await request.get(`/api/drafts/${other.draftId}`)).ok()).toBeTruthy();
  expect((await request.get(`/d/${id}/v/2/raw`)).status()).toBe(404);
  expect(await (await request.get(`/d/${id}/v/1/raw`)).text()).toContain('Latest content');

  const next = await upload(request, id, 'Latest revision');
  expect(next.versionNumber).toBe(2);
  await expect(page.locator('#reset-history')).toBeVisible();
  await page.locator('#reset-history').click();
  await expect(confirm(page)).toBeEnabled();
  await confirm(page).click();
  await expect(page.locator('#reset-history')).toBeHidden();
  await expect(page.locator('#detail-version')).toHaveText('v1 · 1 version');
  await observer.close();
});

test('stale confirmation reloads history and failures stay in the dialog without changing the target', async ({ page, request }) => {
  const first = await upload(request);
  const id = first.draftId;
  await upload(request, id);
  await page.goto(`/?draft=${id}`);
  await page.locator('#reset-history').click();
  await expect(confirm(page)).toBeEnabled();
  await upload(request, id, 'New arrival');
  await confirm(page).click();
  await expect(dialog(page)).toBeVisible();
  await expect(page.locator('#reset-history-error')).toContainText('latest version has changed');
  await expect(page.locator('#reset-history-description')).toHaveText('Permanently delete 2 older versions and renumber v3 as v1. The next upload will be v2.');
  await expect(confirm(page)).toBeEnabled();

  await page.route(`**/api/drafts/${id}/versions/prune`, route => route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: 'Reset failed for test' }) }));
  await confirm(page).click();
  await expect(page.locator('#reset-history-error')).toHaveText('Reset failed for test');
  await expect(dialog(page)).toBeVisible();
  await expect(confirm(page)).toBeEnabled();
  expect((await (await request.get(`/api/drafts/${id}`)).json()).draft.versions).toHaveLength(3);
  await page.unroute(`**/api/drafts/${id}/versions/prune`);
  await confirm(page).click();
  await expect(dialog(page)).not.toBeVisible();
  await expect(page.locator('#version-history .history-item')).toHaveCount(1);
  await expect(page.locator('#detail-title')).toHaveText('New arrival');
});
