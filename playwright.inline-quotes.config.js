import { defineConfig } from '@playwright/test';
import base from './playwright.config.js';

// This suite asserts actual new-tab navigation after a middle click. Use the
// pinned full Chromium channel, as the backlink browser-lifecycle suite does.
// Keep the shared server, request isolation, deadlines and assertions intact.
export default defineConfig({
  ...base,
  use: { ...base.use, channel: 'chromium' },
});
