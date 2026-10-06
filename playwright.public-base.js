import { defineConfig } from '@playwright/test';
import { prepareDeletionQuotaRun } from './tests/browser/helpers/deletion-quota-fixture.js';

export function publicConfig({ visual = process.env.VISUAL_FIXTURE_SERVER === '1' } = {}) {
  if (visual) process.env.VISUAL_FIXTURE_SERVER = '1';
  const quotaRun = visual ? undefined : prepareDeletionQuotaRun();
  return defineConfig({
    globalSetup: visual ? undefined : './tests/browser/helpers/deletion-quota-setup.js',
    globalTeardown: visual ? undefined : './tests/browser/helpers/deletion-quota-teardown.js',
    testDir: './tests/browser',
    testIgnore: '**/staff.spec.js',
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
      env: {
        // Only the disposable browser server receives this per-run identity key.
        POSTER_ID_KEY: quotaRun?.key || '',
        BROWSER_DELETION_QUOTA_MANIFEST: '',
        STAFF_TRIPCODE_KEY: '', STAFF_POSTER_ID_KEY: '', STAFF_COUNTRY_DATABASE: '',
        STAFF_PROXY_SOCKET: '', STAFF_PROXY_UID: '', MEDIA_INTAKE_TOKEN: '',
        MIGRATION_DATABASE_URL: '', STAFF_DATABASE_URL: '', AUTH_DATABASE_URL: '', MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', MONITOR_DATABASE_URL: '', INTAKE_DATABASE_URL: '',
        API_ORIGIN: 'http://127.0.0.1:3003', API_BIND_ADDR: '127.0.0.1:3003',
        // The full browser suite shares one socket peer, including setup and cleanup.
        // The real default and non-default enforcement stay covered by http_limits.rs.
        PUBLIC_WRITES_PER_MINUTE: '1000',
      },
      command: visual
        ? 'cargo run -p board-public --example visual-fixtures --locked'
        : 'cargo run -p board-public --locked',
      url: 'http://127.0.0.1:3000/readyz',
      reuseExistingServer: false,
      timeout: 120_000,
    },
  });
}
