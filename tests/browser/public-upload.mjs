// Real HTML forms only. The supervising fixture supplies isolated processing;
// neither this browser nor the public service gets coordinator credentials.
import assert from 'node:assert/strict';
import path from 'node:path';
import { createHash } from 'node:crypto';
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
  await expect(page.getByLabel('File', { exact: true })).toHaveAttribute('accept', 'image/png,image/jpeg');
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
  await expect(page.locator('table#postForm')).toHaveAttribute('role', 'presentation');
  await expect(page.getByLabel('Comment', { exact: true })).toHaveAttribute('aria-describedby', 'postHelp');
  for (const viewport of [{ width: 390, height: 844 }, { width: 1280, height: 900 }]) {
    await page.setViewportSize(viewport);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await expect(page.getByRole('button', { name: 'Post with image', exact: true })).toBeVisible();
  }
  await page.getByLabel('Name', { exact: true }).fill('Synthetic browser');
  await page.getByLabel('Subject', { exact: true }).fill('Isolated upload');
  await page.getByLabel('Comment', { exact: true }).fill('A synthetic one-pixel image, posted without site JavaScript.');
  await page.getByLabel('Deletion password', { exact: true }).fill('synthetic-browser-password');
  await page.getByRole('button', { name: 'Post with image', exact: true }).click();
  const threadUrl = page.url();
  assert.match(new URL(threadUrl).pathname, new RegExp(`^/${board}/thread/[0-9]+$`));
  const img = page.locator('.fileThumb img');
  await img.scrollIntoViewIfNeeded();
  await expect.poll(() => img.evaluate(image => image.naturalWidth)).toBe(1);
  assert.equal(await img.evaluate(image => image.naturalHeight), 1);
  const thumbnailUrl = new URL(await img.getAttribute('src'));
  const mediaUrl = new URL(await page.locator('.fileThumb').getAttribute('href'));
  assert.notEqual(mediaUrl.origin, origin.origin);
  assert.match(mediaUrl.pathname, new RegExp(`^/${board}/[0-9]+\\.png$`));
  assert.equal(thumbnailUrl.pathname, mediaUrl.pathname.replace('.png', 's.jpg'));
  const media = await page.request.get(mediaUrl.href);
  await screenshot('attached-thread');
  assert.equal(media.status(), 200);
  assert.equal(media.headers()['content-type'], 'image/png');
  assert.equal(media.headers()['cross-origin-resource-policy'], 'cross-origin');
  const etag = media.headers().etag;
  assert.ok(etag);
  const thumbnail = await page.request.get(thumbnailUrl.href);
  assert.equal(thumbnail.status(), 200);
  assert.equal(thumbnail.headers()['content-type'], 'image/png');
  const apiUrl = new URL(threadUrl);
  apiUrl.pathname += '.json';
  apiUrl.hash = '';
  const api = await page.request.get(apiUrl.href);
  assert.equal(api.status(), 200);
  const post = (await api.json()).posts[0];
  assert.equal(post.ext, '.png');
  assert.equal(post.tim, Number(mediaUrl.pathname.split('/').at(-1).replace('.png', '')));
  assert.equal(post.md5, createHash('md5').update(await media.body()).digest('base64'));
  assert.equal(post.fsize, (await media.body()).length);
  assert.deepEqual([post.w, post.h, post.tn_w, post.tn_h, post.images], [1, 1, 1, 1, 0]);
  for (const url of [mediaUrl, thumbnailUrl]) {
    const head = await page.request.head(url.href);
    assert.equal(head.status(), 200);
    assert.equal(head.headers()['content-type'], 'image/png');
    assert.equal((await head.body()).length, 0);
    assert.equal((await page.request.get(url.href, { headers: { 'If-None-Match': head.headers().etag } })).status(), 304);
    for (const path of [url.pathname.replace(`/${board}/`, '/wrongboard/'), url.pathname.replace(`/${board}/`, `/${board}/0`), url.pathname.replace(`/${board}/`, `/${board}/%31`)]) {
      assert.equal((await page.request.get(new URL(path, url.origin).href)).status(), 404);
    }
  }
  for (const suffix of ['', 'catalog']) {
    await page.goto(new URL(`/${board}/${suffix}`, origin).href);
    assert.equal(await page.locator(suffix === 'catalog' ? `#thread-${post.no} .catalogThumb img` : `#p${post.no} .fileThumb img`).getAttribute('src'), thumbnailUrl.href);
    if (suffix === 'catalog') {
      assert.equal(await page.locator(`#thread-${post.no} .catalogThumb`).getAttribute('href'), `/${board}/thread/${post.no}`);
      assert.equal(await page.locator(`#meta-${post.no}`).innerText(), 'R: 0');
    }
    if (suffix === 'catalog') await screenshot('attached-catalog');
  }
  await page.goto(threadUrl);
  await page.getByText('Delete or report', { exact: true }).click();
  const deletion = page.locator(`form[action="/${board}/delete"]`);
  await deletion.getByLabel('Deletion password', { exact: true }).fill('synthetic-browser-password');
  await deletion.getByLabel('File only', { exact: true }).check();
  await deletion.getByRole('button', { name: 'Delete post', exact: true }).click();
  assert.ok(Number.isSafeInteger(post.no) && post.no > 0);
  await expect(page.locator(`#p${post.no}`).getByText('File deleted.', { exact: true })).toBeVisible();
  await screenshot('file-deleted');
  assert.equal(await page.locator('.fileThumb img').count(), 0);
  const removed = await page.request.get(mediaUrl.href, { headers: { 'If-None-Match': etag } });
  assert.equal(removed.status(), 404, 'deletion must override an old successful validator');
  assert.equal((await page.request.get(thumbnailUrl.href, { headers: { 'If-None-Match': thumbnail.headers().etag } })).status(), 404);
  const deleted = (await (await page.request.get(apiUrl.href)).json()).posts[0];
  assert.equal(deleted.filedeleted, 1);
  for (const field of ['tim', 'md5', 'ext', 'fsize', 'tn_w', 'tn_h']) assert.equal(deleted[field], undefined);
  const deletedRequests = [];
  page.on('request', request => {
    if ([mediaUrl.href, thumbnailUrl.href].includes(request.url())) deletedRequests.push(request.url());
  });
  await page.goto(new URL(`/${board}/catalog`, origin).href);
  const placeholder = page.locator(`#thread-${post.no} .catalogThumb img`);
  await expect(placeholder).toHaveAttribute('src', '/static/catalog/filedeleted-res.gif');
  await expect(placeholder).toHaveAttribute('alt', 'File deleted.');
  await placeholder.scrollIntoViewIfNeeded();
  await expect.poll(() => placeholder.evaluate(img => img.complete && img.naturalWidth === 127)).toBe(true);
  const placeholderBox = await placeholder.boundingBox();
  assert.deepEqual([placeholderBox.width, placeholderBox.height], [155, 53]);
  assert.equal(deletedRequests.length, 0, 'a deleted catalog file loads only the fixed UI asset');
  assert.equal((await context.cookies()).length, 0);
  console.log('PASS no-JavaScript upload, isolated approval, persisted posting, image rendering and file-only deletion');
} finally {
  if (browser) await browser.close();
}
