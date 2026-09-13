// Real HTML forms only. The supervising fixture supplies isolated processing;
// neither this browser nor the public service gets coordinator credentials.
import assert from 'node:assert/strict';
import path from 'node:path';
import { chromium, expect } from '@playwright/test';

const origin = new URL(process.argv[2]);
const board = process.argv[3];
const source = process.argv[4];
const screenshots = process.env.PUBLIC_UPLOAD_SCREENSHOTS;
if (screenshots) assert.ok(path.isAbsolute(screenshots));
assert.equal(origin.hostname, '127.0.0.1');
assert.equal(origin.protocol, 'http:');
assert.match(board, /^[a-z0-9]{1,10}$/);
let browser;
let cancelled = false;
const cancel = () => {
  cancelled = true;
  if (browser) void browser.close();
};
process.once('SIGTERM', cancel);
process.once('SIGINT', cancel);
try {
  browser = await chromium.launch({ timeout: 15_000 });
  assert.equal(cancelled, false, 'browser qualification canceled');
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const screenshot = async name => {
    if (screenshots) {
      await page.screenshot({ path: path.join(screenshots, `${name}.png`), fullPage: true });
      await page.setViewportSize({ width: 390, height: 844 });
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
      await page.screenshot({ path: path.join(screenshots, `${name}-mobile.png`), fullPage: true });
      await page.setViewportSize({ width: 1280, height: 900 });
    }
  };
  page.setDefaultTimeout(10_000);
  await page.goto(new URL(`/${board}/`, origin).href);
  await page.getByLabel('File', { exact: true }).setInputFiles(source);
  await page.getByRole('button', { name: 'Upload file', exact: true }).click();
  assert.equal(new URL(page.url()).pathname, `/${board}/upload`);
  assert.equal(new URL(page.url()).search, '');
  await expect(page.getByRole('status')).toContainText('queued');
  await screenshot('upload-status');
  // The supervisor observes the actual queue and dispatches its one job.
  // Polling is test automation of the visible no-JavaScript status button.
  const deadline = Date.now() + 45_000;
  while (await page.getByRole('button', { name: 'Check upload status', exact: true }).count()) {
    assert.ok(Date.now() < deadline, 'isolated approval deadline exceeded');
    const response = page.waitForResponse(r => new URL(r.url()).pathname === `/${board}/upload/status`);
    await page.getByRole('button', { name: 'Check upload status', exact: true }).click();
    const status = await response;
    assert.equal(status.status(), 200);
    assert.equal(status.headers()['cache-control'], 'private, no-store');
    assert.equal(status.headers()['set-cookie'], undefined);
    if (await page.getByRole('button', { name: 'Post with image', exact: true }).count()) break;
    // Keep a slow processing job within the public 30-writes/minute policy.
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  await screenshot('approved-post-form');
  await page.getByLabel('Name', { exact: true }).fill('Synthetic browser');
  await page.getByLabel('Subject', { exact: true }).fill('Isolated upload');
  await page.getByLabel('Comment', { exact: true }).fill('A synthetic one-pixel PNG, posted without site JavaScript.');
  await page.getByLabel('Deletion password', { exact: true }).fill('synthetic-browser-password');
  await page.getByRole('button', { name: 'Post with image', exact: true }).click();
  const threadUrl = page.url();
  assert.match(new URL(threadUrl).pathname, new RegExp(`^/${board}/thread/[0-9]+$`));
  const img = page.locator('.fileThumb img');
  await img.scrollIntoViewIfNeeded();
  await expect.poll(() => img.evaluate(image => image.naturalWidth)).toBe(1);
  assert.equal(await img.evaluate(image => image.naturalHeight), 1);
  const mediaUrl = new URL(await img.getAttribute('src'));
  assert.notEqual(mediaUrl.origin, origin.origin);
  assert.match(mediaUrl.pathname, /^\/media\/[a-f0-9]{32}\.png$/);
  const media = await page.request.get(mediaUrl.href);
  await screenshot('attached-thread');
  assert.equal(media.status(), 200);
  assert.equal(media.headers()['content-type'], 'image/png');
  assert.equal(media.headers()['cross-origin-resource-policy'], 'cross-origin');
  const etag = media.headers().etag;
  assert.ok(etag);
  for (const suffix of ['', 'catalog']) {
    await page.goto(new URL(`/${board}/${suffix}`, origin).href);
    assert.equal(await page.locator('.fileThumb img').getAttribute('src'), mediaUrl.href);
    if (suffix === 'catalog') await screenshot('attached-catalog');
  }
  await page.goto(threadUrl);
  await page.getByText('Delete or report', { exact: true }).click();
  const deletion = page.locator(`form[action="/${board}/delete"]`);
  await deletion.getByLabel('Deletion password', { exact: true }).fill('synthetic-browser-password');
  await deletion.getByLabel('File only', { exact: true }).check();
  await deletion.getByRole('button', { name: 'Delete post', exact: true }).click();
  await expect(page.getByText('File deleted.', { exact: true })).toBeVisible();
  await screenshot('file-deleted');
  assert.equal(await page.locator('.fileThumb img').count(), 0);
  const removed = await page.request.get(mediaUrl.href, { headers: { 'If-None-Match': etag } });
  assert.equal(removed.status(), 404, 'deletion must override an old successful validator');
  assert.equal((await context.cookies()).length, 0);
  console.log('PASS no-JavaScript upload, isolated approval, persisted posting, image rendering and file-only deletion');
} finally {
  if (browser) await browser.close();
}
