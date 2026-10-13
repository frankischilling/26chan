import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir:'./tests/replay-worker', testMatch:['full-runtime.spec.js','session.spec.js','player.spec.js'], workers:1, retries:0, timeout:30000,
  use:{baseURL:'http://127.0.0.1:8790'}, projects:[{name:'chromium',use:{browserName:'chromium'}}],
  webServer:{command:'node tests/fixtures/replay-worker/full-serve.mjs',url:'http://127.0.0.1:8790',reuseExistingServer:false,timeout:10000},
});
