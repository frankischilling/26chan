import { defineConfig } from '@playwright/test';
import base from './playwright.custom-spoilers.config.js';
export default defineConfig({ ...base, testDir: './tests/source-flags' });
