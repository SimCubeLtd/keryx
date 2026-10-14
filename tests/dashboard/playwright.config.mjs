import { defineConfig } from '../../frontend/node_modules/@playwright/test/index.mjs';
const origin = `http://127.0.0.1:${process.env.KERYX_TEST_PORT || '17823'}`;
export default defineConfig({
  testDir: '.', testMatch: '*.spec.mjs', workers: 1,
  outputDir: '/tmp/keryx-tags-browser-results',
  use: { baseURL: origin, serviceWorkers: 'allow', launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } },
  webServer: {
    command: 'node tests/dashboard/server.mjs', cwd: new URL('../..', import.meta.url).pathname,
    url: `${origin}/healthz`, reuseExistingServer: false,
  },
});
