import { defineConfig } from '@playwright/test';
// In-memory HTTP fixtures exercise the shipped assets without database writes.
export default defineConfig({
  testDir: './tests/browser', testMatch: 'native-blotter.spec.js',
  fullyParallel: true, retries: 0,
  use: { browserName: 'chromium', baseURL: 'http://blotter.test', viewport: { width: 1280, height: 900 } },
});
