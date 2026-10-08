import { defineConfig } from '@playwright/test';
import base from './playwright.archive-visual.config.js';

export default defineConfig({ ...base, testDir: './tests/custom-spoilers',
  use: { ...base.use, javaScriptEnabled: true } });
