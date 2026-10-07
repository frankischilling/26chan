import { defineConfig } from '@playwright/test';
import { publicConfig } from './playwright.public-base.js';

const base = publicConfig({ visual: true });
export default defineConfig({
  ...base,
  testMatch: '**/native-math.spec.js',
  webServer: {
    ...base.webServer,
    command: 'cargo run -p board-public --example math-browser-fixture --features browser-tests --locked',
  },
});
