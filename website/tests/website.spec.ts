import { expect, test } from '@playwright/test';

const base = process.env.SITE_BASE || '';

for (const path of ['/', '/docs/quickstart/']) {
  test(`colour modes follow the system and preserve an explicit choice on ${path}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto(base + path);
    const picker = page.getByRole('button', { name: /^Colour mode:/ });
    await expect(picker).toHaveAccessibleName(/Colour mode: System\./);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await picker.press('Enter');
    await page.reload();
    await expect(picker).toHaveAccessibleName(/Colour mode: Light\./);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await picker.press('Space');
    await page.emulateMedia({ colorScheme: 'light' });
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.goto(base + (path === '/' ? '/docs/quickstart/' : '/'));
    await expect(picker).toHaveAccessibleName(/Colour mode: Dark\./);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await picker.click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.reload();
    await expect(picker).toHaveAccessibleName(/Colour mode: System\./);
    await page.emulateMedia({ colorScheme: 'dark' });
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  });
}

test('theme selection still works when storage is unavailable', async ({ page }) => {
  const errors: Error[] = [];
  page.on('pageerror', (error) => errors.push(error));
  await page.addInitScript(() => {
    Object.defineProperty(window, 'localStorage', { get() { throw new DOMException('Blocked', 'SecurityError'); } });
  });
  await page.goto(base + '/');
  await page.getByRole('button', { name: /^Colour mode:/ }).click();
  await page.getByRole('button', { name: /^Colour mode:/ }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(errors).toEqual([]);
});

test('mobile landing page links to readable docs and working search', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(base + '/');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole('link', { name: 'Publish your first document' }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Publish your first document');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.getByRole('button', { name: /^Colour mode:/ }).click();
  await page.getByRole('button', { name: /^Colour mode:/ }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.getByRole('textbox', { name: 'Search', exact: true }).fill('PDF');
  await expect(page.locator('.pagefind-ui__result-link').first()).toBeVisible();
});
