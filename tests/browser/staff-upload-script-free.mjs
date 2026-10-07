// Invoked only by the ignored real-role Rust upload browser test. Control data
// stays in anonymous pipes; no credentials/receipts, traces, or state files.
import { chromium } from '@playwright/test';
import assert from 'node:assert/strict';
import readline from 'node:readline';
const lines = readline.createInterface({ input: process.stdin, terminal: false })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext({ javaScriptEnabled: false });
  await context.addCookies([
    { name: 'staff', value: config.token, url: config.origin, httpOnly: true, sameSite: 'Strict' },
    { name: 'staff-csrf', value: config.csrf, url: config.origin, httpOnly: true, sameSite: 'Strict' },
  ]);
  const page = await context.newPage();
  const urls = [];
  page.on('request', request => urls.push(request.url()));
  await page.goto(`${config.origin}/post?board=${config.board}&thread=0`);
  const form = page.locator('form[action="/post/upload"]');
  assert.equal(await form.count(), 1, 'staff upload form missing');
  await form.locator('[name=upfile]').setInputFiles({ name: `${config.board}.png`, mimeType: 'image/png', buffer: Buffer.alloc(256, 42) });
  await Promise.all([page.waitForURL(`${config.origin}/post/upload`), form.getByRole('button', { name: 'Upload file', exact: true }).click()]);
  assert.equal(await page.locator('img').count(), 0, 'unapproved input was previewed');
  assert.equal(await page.locator('script').count(), 0, 'receipt page contains script');
  assert.equal(await page.locator('form[action="/post"]').count(), 0, 'queued upload allowed posting');
  const id = await page.locator('input[name=upload_id]').first().inputValue();
  const capability = await page.locator('input[name=upload_capability]').first().inputValue();
  await Promise.all([page.waitForURL(`${config.origin}/post/upload/status`), page.getByRole('button', { name: 'Check status', exact: true }).click()]);
  assert.equal(await page.locator('img').count(), 0, 'queued status previewed input');
  // The parent uses its coordinator role to approve only this owned synthetic job.
  process.stdout.write(`${JSON.stringify({ upload: id })}\n`);
  assert.equal((await lines.next()).value, 'approved', 'fixture approval missing');
  await Promise.all([page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/post/upload/status')), page.getByRole('button', { name: 'Check status', exact: true }).click()]);
  await page.locator('form[action="/post"]').waitFor();
  await page.getByLabel('Staff badge', { exact: true }).selectOption('mod');
  await page.getByLabel('Name', { exact: true }).fill('Owned browser');
  await page.getByLabel('Subject', { exact: true }).fill('Owned script-free attachment');
  await page.getByLabel('Comment', { exact: true }).fill('Owned script-free staff upload');
  await page.getByLabel('Spoiler image', { exact: true }).check();
  await Promise.all([page.waitForURL(url => url.pathname === '/post' && url.searchParams.has('posted')), page.getByRole('button', { name: 'Post', exact: true }).click()]);
  assert.ok(urls.every(url => !url.includes(id) && !url.includes(capability)), 'receipt entered a request URL');
  assert.equal(await page.evaluate(() => localStorage.length + sessionStorage.length), 0, 'receipt workflow wrote browser storage');
  // Exercise cancellation through another actual no-script form submission.
  await page.goto(`${config.origin}/post?board=${config.board}&thread=0`);
  const cancelForm = page.locator('form[action="/post/upload"]');
  await cancelForm.locator('[name=upfile]').setInputFiles({ name: `${config.board}.png`, mimeType: 'image/png', buffer: Buffer.alloc(256, 42) });
  await Promise.all([page.waitForURL(`${config.origin}/post/upload`), cancelForm.getByRole('button', { name: 'Upload file', exact: true }).click()]);
  await Promise.all([page.waitForURL(url => url.pathname === '/post' && url.searchParams.get('board') === config.board), page.getByRole('button', { name: 'Cancel upload', exact: true }).click()]);
  await context.close();
} finally {
  await browser.close();
  process.stdin.destroy();
}
