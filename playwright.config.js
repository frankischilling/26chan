import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30_000,
  expect: { toHaveScreenshot: { maxDiffPixels: 0, animations: 'disabled' } },
  use: {
    baseURL: 'http://127.0.0.1:3000',
    browserName: 'chromium',
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
    locale: 'en-US',
    timezoneId: 'America/New_York',
    colorScheme: 'light',
    trace: 'retain-on-failure',
  },
  webServer: {
    // Test runner needs migration credentials for DB checks; the spawned public
    // process receives neither those credentials nor future staff credentials.
    env: { MIGRATION_DATABASE_URL: '', STAFF_DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '' },
    command: process.env.VISUAL_FIXTURE_SERVER === '1'
      ? 'cargo run -p board-public --example visual-fixtures --locked'
      : 'cargo run -p board-public --locked',
    url: 'http://127.0.0.1:3000/readyz',
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
