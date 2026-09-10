import assert from 'node:assert/strict';
import http from 'node:http';
import { chromium } from '@playwright/test';

const media = new URL(process.argv[2]);
assert.equal(media.hostname, '127.0.0.1');
assert.match(media.pathname, /^\/media\/[a-f0-9]{32}\.png$/);
const wrapper = http.createServer((_request, response) => {
  response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
  response.end(`<!doctype html><title>Synthetic media fixture</title><img id="fixture" alt="Synthetic pixel" src="${media.href}">`);
});
await new Promise(resolve => wrapper.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await chromium.launch({ timeout: 15_000 });
  const page = await browser.newPage({ viewport: { width: 640, height: 480 } });
  page.setDefaultTimeout(10_000);
  await page.goto(`http://127.0.0.1:${wrapper.address().port}/`);
  await page.waitForFunction(() => document.querySelector('#fixture')?.naturalWidth === 1);
  assert.equal(await page.locator('#fixture').evaluate(image => image.naturalHeight), 1);
  const response = await page.request.get(media.href);
  assert.equal(response.status(), 200);
  const headers = response.headers();
  assert.equal(headers['content-type'], 'image/png');
  assert.equal(headers['cross-origin-resource-policy'], 'cross-origin');
  assert.equal(headers['x-content-type-options'], 'nosniff');
  assert.equal(headers['set-cookie'], undefined);
  const bytes = await response.body();
  assert.deepEqual(bytes.subarray(0, 8), Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
  const cookie = await page.request.get(media.href, { headers: { Cookie: 'synthetic_staff_cookie=untrusted' } });
  assert.deepEqual(await cookie.body(), bytes);
  assert.equal(cookie.headers()['set-cookie'], undefined);
  const conditional = await page.request.get(media.href, { headers: { 'If-None-Match': headers.etag } });
  assert.equal(conditional.status(), 304);
  assert.equal((await conditional.body()).length, 0);
  await page.goto(media.href);
  await page.waitForFunction(() => document.querySelector('img')?.naturalWidth === 1);
  console.log('PASS browser cross-origin embedding, direct PNG display, cookie-independent bytes and conditional read');
} finally {
  if (browser) await browser.close();
  await new Promise(resolve => wrapper.close(resolve));
}
