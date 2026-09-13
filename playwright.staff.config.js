import { defineConfig } from '@playwright/test';
import path from 'node:path';
const binary = process.platform === 'win32' ? '.exe' : '';
const shared = { STAFF_MODE: 'development', STAFF_ORIGIN: 'http://localhost:3001', STAFF_IDLE_TIMEOUT_SECONDS: '900', PUBLIC_ORIGIN: 'http://127.0.0.1:3000', MEDIA_ORIGIN: 'http://127.0.0.1:3002' };
export default defineConfig({
  testDir: './tests/browser', testMatch: '**/staff.spec.js', workers: 1, retries: 0, timeout: 90_000,
  use: { baseURL: 'http://localhost:3001', browserName: 'chromium', trace: 'off', screenshot: 'off', video: 'off' },
  webServer: [
    { command: `"${path.resolve(`target/debug/board-staff${binary}`)}"`, url: 'http://localhost:3001/readyz', timeout: 30_000, reuseExistingServer: false,
      env: { ...shared, STAFF_BIND: '127.0.0.1:3001', MIGRATION_DATABASE_URL: '', DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', MONITOR_DATABASE_URL: '', INTAKE_DATABASE_URL: '' } },
    { command: `"${path.resolve(`target/debug/board-public${binary}`)}"`, url: 'http://127.0.0.1:3000/readyz', timeout: 30_000, reuseExistingServer: false,
      env: { ...shared, BIND_ADDR: '127.0.0.1:3000', APP_ENV: 'development', MEDIA_ENABLED: 'false', MIGRATION_DATABASE_URL: '', AUTH_DATABASE_URL: '', STAFF_DATABASE_URL: '', MEDIA_DATABASE_URL: '', MEDIA_READ_DATABASE_URL: '', TEST_PUBLIC_DATABASE_URL: '', MONITOR_DATABASE_URL: '', INTAKE_DATABASE_URL: '' } },
  ],
});
