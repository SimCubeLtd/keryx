import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  use: {
    baseURL: 'http://127.0.0.1:4329',
    launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE },
  },
  webServer: {
    command: 'node tests/preview.mjs',
    url: 'http://127.0.0.1:4329' + (process.env.SITE_BASE || '') + '/',
    reuseExistingServer: false,
  },
});
