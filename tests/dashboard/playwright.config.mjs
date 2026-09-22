import { defineConfig } from '../../website/node_modules/@playwright/test/index.mjs';
export default defineConfig({
  testDir: '.', testMatch: '*.spec.mjs', workers: 1,
  outputDir: '/tmp/keryx-tags-browser-results',
  use: { baseURL: 'http://127.0.0.1:17823', serviceWorkers: 'block', launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } },
  webServer: {
    command: 'node tests/dashboard/server.mjs', cwd: new URL('../..', import.meta.url).pathname,
    url: 'http://127.0.0.1:17823/healthz', reuseExistingServer: false,
  },
});
