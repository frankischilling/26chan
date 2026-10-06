import { defineConfig } from '@playwright/test';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { serviceCredentialOverrides } from './tests/browser/staff-service-environment.mjs';
import { prepareDeletionQuotaRun } from './tests/browser/helpers/deletion-quota-fixture.js';
// Prepare identity only in memory; setup creates its ownership proof after servers start.
const quotaRun = prepareDeletionQuotaRun();
const binary = process.platform === 'win32' ? '.exe' : '';
const debugDir = path.resolve(process.env.CARGO_TARGET_DIR || 'target', 'debug');
const countryDatabase = path.resolve('crates/domain/tests/fixtures/GeoIP2-Country-Test.mmdb');
const intakeToken = randomBytes(32).toString('hex');
const intakeRoot = path.resolve(`.local/staff-browser-intake-${randomBytes(8).toString('hex')}`);
const shared = { STAFF_MODE: 'development', STAFF_ORIGIN: 'http://localhost:3001', STAFF_IDLE_TIMEOUT_SECONDS: '900', PUBLIC_ORIGIN: 'http://127.0.0.1:3000', MEDIA_ORIGIN: 'http://127.0.0.2:3002' };
export default defineConfig({
  testDir: './tests/browser', testMatch: '**/staff.spec.js', fullyParallel: false, workers: 1, retries: 0, timeout: 90_000,
  metadata: { ownedStaffIntakeRoot: intakeRoot },
  globalSetup: './tests/browser/helpers/deletion-quota-setup.js',
  globalTeardown: './tests/browser/staff-teardown.js',
  use: { baseURL: 'http://localhost:3001', browserName: 'chromium', trace: 'off', screenshot: 'off', video: 'off' },
  webServer: [
    { command: `"${path.join(debugDir, `board-media-intake${binary}`)}"`, url: 'http://127.0.0.1:3004/readyz', timeout: 30_000, reuseExistingServer: false,
      env: { ...serviceCredentialOverrides(process.env, ['INTAKE_DATABASE_URL']), MEDIA_INTAKE_MODE: 'development', MEDIA_ENABLED: 'false', MEDIA_INTAKE_BIND: '127.0.0.1:3004', MEDIA_INTAKE_TOKEN: intakeToken, MEDIA_QUARANTINE_DIR: intakeRoot,
        BROWSER_DELETION_QUOTA_MANIFEST: '', MIGRATION_DATABASE_URL: '', DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', AUTH_DATABASE_URL: '', STAFF_DATABASE_URL: '', MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', MONITOR_DATABASE_URL: '',
        STAFF_TRIPCODE_KEY: undefined, STAFF_POSTER_ID_KEY: undefined, STAFF_COUNTRY_DATABASE: undefined, STAFF_PROXY_SOCKET: undefined, STAFF_PROXY_UID: undefined,
        PUBLIC_INTAKE_TOKEN: '', GH_TOKEN: '', GITHUB_TOKEN: '', TRIPCODE_KEY: undefined, POSTER_ID_KEY: undefined, COUNTRY_DATABASE: undefined } },
    { command: `"${path.join(debugDir, `board-staff${binary}`)}"`, url: 'http://localhost:3001/readyz', timeout: 30_000, reuseExistingServer: false,
      env: { ...serviceCredentialOverrides(process.env, ['AUTH_DATABASE_URL', 'STAFF_DATABASE_URL']), ...shared, STAFF_BIND: '127.0.0.1:3001', STAFF_TRIPCODE_KEY: '11'.repeat(32), STAFF_POSTER_ID_KEY: quotaRun.key, STAFF_COUNTRY_DATABASE: countryDatabase, STAFF_PROXY_SOCKET: undefined, STAFF_PROXY_UID: undefined, BROWSER_DELETION_QUOTA_MANIFEST: '', MIGRATION_DATABASE_URL: '', DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', MONITOR_DATABASE_URL: '', INTAKE_DATABASE_URL: '', TRIPCODE_KEY: '', POSTER_ID_KEY: '', COUNTRY_DATABASE: '' } },
    { command: `"${path.join(debugDir, `board-public${binary}`)}"`, url: 'http://127.0.0.1:3000/readyz', timeout: 30_000, reuseExistingServer: false,
      env: { ...serviceCredentialOverrides(process.env, ['DATABASE_URL']), ...shared, BIND_ADDR: '127.0.0.1:3000', APP_ENV: 'development', MEDIA_ENABLED: 'true', PUBLIC_MEDIA_PROFILE: 'isolated-development', PUBLIC_INTAKE_ADDR: '127.0.0.1:3004', PUBLIC_INTAKE_TOKEN: intakeToken,
        BROWSER_DELETION_QUOTA_MANIFEST: '', MIGRATION_DATABASE_URL: '', AUTH_DATABASE_URL: '', STAFF_DATABASE_URL: '', STAFF_TRIPCODE_KEY: undefined, STAFF_POSTER_ID_KEY: undefined, STAFF_COUNTRY_DATABASE: undefined, STAFF_PROXY_SOCKET: undefined, STAFF_PROXY_UID: undefined, MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', MONITOR_DATABASE_URL: '', INTAKE_DATABASE_URL: '', TRIPCODE_KEY: undefined, POSTER_ID_KEY: quotaRun.key, COUNTRY_DATABASE: countryDatabase } },
  ],
});
