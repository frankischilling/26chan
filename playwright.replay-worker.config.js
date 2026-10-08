import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/replay-worker', testMatch: 'native.spec.js',
  timeout: 120000, expect: { timeout: 10000 }, workers: 1, retries: 0,
  use: { baseURL: 'http://127.0.0.1:8789', viewport: { width: 1200, height: 900 }, trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  webServer: { command: 'node tests/fixtures/replay-worker/serve.mjs', url: 'http://127.0.0.1:8789', reuseExistingServer: false, timeout: 10000 },
});
