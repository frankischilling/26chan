import { defineConfig } from '@playwright/test';
import { publicConfig } from './playwright.public-base.js';

// Persisted PostgreSQL fixtures and the actual public HTTP application.
export default defineConfig({
  ...publicConfig({ visual: false }),
  testMatch: 'source-board-subtitles.spec.js',
  timeout: 120000,
});
