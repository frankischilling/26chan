import { defineConfig } from '@playwright/test';
import { publicConfig } from './playwright.public-base.js';
const base = publicConfig({ visual: true });

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
