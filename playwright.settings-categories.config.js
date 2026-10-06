import { defineConfig } from '@playwright/test';

// Module-only fixtures intercept every request and need no application or DB.
export default defineConfig({
  testDir: './tests/browser',
  testMatch: 'native-settings-categories.spec.js',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30_000,
  use: {
    browserName: 'chromium',
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
    locale: 'en-US',
    timezoneId: 'America/New_York',
    trace: 'retain-on-failure',
  },
});
