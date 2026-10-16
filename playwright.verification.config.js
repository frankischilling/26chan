import { defineConfig } from '@playwright/test';
import { publicConfig } from './playwright.public-base.js';
process.env.VERIFICATION_BROWSER_FIXTURE = "1";
const base = publicConfig({ visual: true });
export default defineConfig({ ...base, testMatch: '**/public-verification.spec.js',
  webServer: { ...base.webServer, command: 'cargo run -p board-public --example verification-browser-fixture --features browser-tests --locked' },
});
