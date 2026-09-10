import { defineConfig } from '@playwright/test';
import base from './playwright.config.js';

export default defineConfig({
  ...base,
  testDir: './tests/archive-visual',
  testIgnore: [],
  use: { ...base.use, javaScriptEnabled: false, screenshot: 'only-on-failure' },
  webServer: {
    ...base.webServer,
    command: 'cargo run -p board-public --example visual-fixtures --locked',
  },
});
