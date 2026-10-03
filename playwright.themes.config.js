import { defineConfig } from '@playwright/test';
import base from './playwright.archive-visual.config.js';

export default defineConfig({
  ...base,
  testDir: './tests/themes',
  ...(process.platform === 'win32' && process.env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS === '1'
    ? { reporter: [[process.env.CI ? 'dot' : 'list'], ['./scripts/windows-visual-resource-reporter.mjs']] }
    : {}),
});
