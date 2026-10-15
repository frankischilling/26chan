// Real source-board HTML forms with site JavaScript disabled. Synthetic data only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { chromium, expect } from '@playwright/test';

const origin = new URL(process.argv[2]), source = process.argv[3];
assert.equal(origin.hostname, '127.0.0.1');
assert.equal(origin.protocol, 'http:');
for (const key of Object.keys(process.env)) assert.ok(!key.endsWith('DATABASE_URL') && !['POSTER_ID_KEY', 'PUBLIC_INTAKE_TOKEN'].includes(key));
const original = await readFile(source);
let browser;
const cancel = () => { void browser?.close(); };
process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
try {
  browser = await chromium.launch({ timeout: 15_000 });
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  page.setDefaultTimeout(10_000);
  await page.goto(new URL('/gif/', origin).href);
  await page.getByLabel('File', { exact: true }).setInputFiles(source);
  await page.getByRole('button', { name: 'Upload file', exact: true }).click();
  const deadline = Date.now() + 70_000;
  while (!await page.getByRole('button', { name: 'Post with image', exact: true }).count()) {
    assert.ok(Date.now() < deadline, 'source GIF approval deadline');
    await page.getByRole('button', { name: 'Check upload status', exact: true }).click();
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  await page.getByLabel('Subject', { exact: true }).fill('Owned source GIF qualification');
  await page.getByLabel('Comment', { exact: true }).fill('Synthetic animated red and blue pixels.');
  const posted = page.waitForResponse(r => r.request().method() === 'POST' && new URL(r.url()).pathname === '/gif/imgboard.php');
  await page.getByRole('button', { name: 'Post with image', exact: true }).click();
  assert.equal((await posted).status(), 303);
  await expect(page).toHaveURL(/\/gif\/thread\/[1-9][0-9]*/);
  const threadUrl = page.url(), thread = new URL(threadUrl).pathname.split('/').at(-1);
  const image = page.locator(`#p${thread} .fileThumb img`);
  await expect.poll(() => image.evaluate(img => img.naturalWidth)).toBe(1);
  const thumbnail = new URL(await image.getAttribute('src'));
  const full = new URL(await page.locator(`#p${thread} .fileThumb`).getAttribute('href'));
  assert.notEqual(full.origin, origin.origin);
  const response = await page.request.get(full.href);
  assert.equal(response.headers()['content-type'], 'image/gif');
  assert.deepEqual(await response.body(), original);
  const thumbResponse = await page.request.get(thumbnail.href);
  assert.equal(thumbResponse.headers()['content-type'], 'image/jpeg');
  const api = new URL(threadUrl); api.hash = ''; api.pathname += '.json';
  const post = (await (await page.request.get(api.href)).json()).posts[0];
  assert.equal(post.ext, '.gif');
  assert.equal(post.md5, createHash('md5').update(original).digest('base64'));
  assert.equal(post.fsize, original.length);
  assert.equal(post.m_img, undefined);
  assert.deepEqual([post.w, post.h, post.tn_w, post.tn_h], [1, 1, 1, 1]);
  await page.goto(full.href);
  const animation = page.locator('img');
  await expect.poll(() => animation.evaluate(img => img.naturalWidth)).toBe(1);
  const first = await animation.screenshot();
  let changed = false;
  for (let attempt = 0; attempt < 20 && !changed; attempt++) {
    await page.waitForTimeout(60);
    changed = !(await animation.screenshot()).equals(first);
  }
  assert.ok(changed, 'retained source GIF visibly advances with JavaScript disabled');
  // Keep the actual source board's 60-second known-owner deletion threshold.
  await new Promise(resolve => setTimeout(resolve, 62_000));
  await page.goto(threadUrl);
  await page.locator(`#p${thread}`).getByText('Delete or report', { exact: true }).click();
  const form = page.locator(`#p${thread} form[action="/gif/delete"]`);
  await form.getByLabel('File only', { exact: true }).check();
  await form.getByRole('button', { name: 'Delete post', exact: true }).click();
  await expect(page.locator(`#p${thread}`).getByRole('img', { name: 'File deleted.', exact: true })).toBeVisible();
  for (const url of [full, thumbnail]) assert.equal((await page.request.get(url.href)).status(), 404);
  const deleted = (await (await page.request.get(api.href)).json()).posts[0];
  assert.equal(deleted.filedeleted, 1);
  for (const key of ['ext', 'tim', 'md5', 'm_img', 'fsize']) assert.equal(deleted[key], undefined);
  console.log('PASS source GIF real public posting, retained bytes, JPEG thumbnail, API, no-JavaScript animation and owner deletion');
} finally {
  await browser?.close();
}
