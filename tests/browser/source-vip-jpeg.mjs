// Synthetic credentials and bytes on a leased disposable source board. Actual
// intake, issuer, isolated decoder, database writer and reader run independently.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium, expect } from '@playwright/test';
const configBytes = await readFile(process.argv[2]); assert.ok(configBytes.length <= 4096);
const { origin, mediaOrigin, source, credential } = JSON.parse(configBytes);
const address = new URL(origin);
assert.equal(address.protocol, 'http:'); assert.equal(address.hostname, '127.0.0.1');
assert.equal(address.origin, origin);
assert.equal(new URL(mediaOrigin).protocol, 'http:'); assert.equal(new URL(mediaOrigin).hostname, '127.0.0.1');
assert.equal(path.dirname(path.resolve(source)), path.dirname(path.resolve(process.argv[2])));
assert.match(path.basename(source), /^public-vip-jpeg-[0-9a-f]{32}\.jpg$/);
assert.match(credential, /^capcode_!ownedjpeg-[0-9a-f]{16}![0-9a-f]{32}$/);
for (const key of Object.keys(process.env)) assert.ok(!key.endsWith('DATABASE_URL') && !/TOKEN|KEY/.test(key));
let browser, stage = 'launch';
let cancelled = false;
const cancel = () => { cancelled = true; void browser?.close().catch(() => {}); };
process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
const lifetime = setTimeout(() => { stage = 'deadline'; cancel(); }, 80_000);
try {
  browser = await chromium.launch({ timeout: 15000 });
  assert.ok(!cancelled, 'owned browser cancelled');
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage(); page.setDefaultTimeout(12000);
  stage = 'upload';
  await page.goto(`${origin}/p/`);
  await page.locator('[name=vip_image]').check();
  await page.getByLabel('File', { exact: true }).setInputFiles(source);
  await page.getByRole('button', { name: 'Upload file', exact: true }).click();
  const deadline = Date.now() + 45000;
  while (await page.getByRole('button', { name: 'Check upload status', exact: true }).count()) {
    assert.ok(Date.now() < deadline, 'owned processing deadline');
    await page.getByRole('button', { name: 'Check upload status', exact: true }).click();
    if (await page.locator('form.postEditor').count()) break;
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  stage = 'posting';
  const form = page.locator('form.postEditor');
  await form.locator('[name=email]').fill(credential);
  await form.locator('[name=sub]').fill('Owned Verified JPEG');
  await form.locator('[name=com]').fill('Owned real JPEG verification');
  await Promise.all([page.waitForURL(url => /^\/p\/thread\/[1-9][0-9]*$/.test(url.pathname) && /^#p[1-9][0-9]*$/.test(url.hash)),
    form.getByRole('button', { name: 'Post with image', exact: true }).click()]);
  stage = 'saved-post';
  const id = new URL(page.url()).hash.slice(2), post = page.locator(`#p${id}`);
  await expect(post.locator('.postInfo .name')).toHaveText('Owned Verified JPEG');
  await expect(post.locator('.postInfo .capcode')).toHaveText('## Verified');
  assert.equal(await post.locator('.identityIcon').count(), 0);
  const href = await post.locator('.fileText a').first().getAttribute('href');
  const media = new URL(href, origin); assert.equal(media.origin, mediaOrigin);
  assert.match(media.pathname, /^\/p\/[1-9][0-9]*\.jpg$/);
  const response = await context.request.get(media.href);
  assert.equal(response.status(), 200);
  assert.match(response.headers()['content-type'], /^image\/jpeg/);
  const bytes = await response.body(); assert.ok(bytes.length > 4);
  assert.deepEqual(bytes.subarray(0, 3), Buffer.from([255, 216, 255]));
  assert.ok(!bytes.includes(Buffer.from('OWNED-VIP-JPEG-METADATA')));
  stage = 'preferences';
  const html = await page.content(), [_, identifier, password] = credential.split('!');
  assert.ok(!html.includes(identifier) && !html.includes(password));
  for (const cookie of await context.cookies()) assert.ok(!cookie.value.includes(identifier) && !cookie.value.includes(password));
  process.stdout.write('PASS verified JPEG public form, real isolated processing and metadata stripping\n');
} catch (error) {
  const where = String(error?.stack ?? '').match(/source-vip-jpeg\.mjs:([0-9]{1,6}):([0-9]{1,6})/);
  console.error(`OWNED_VIP_JPEG stage=${stage}${where ? ` line=${where[1]}:${where[2]}` : ''}`);
  process.exitCode = 1;
} finally { clearTimeout(lifetime); await browser?.close(); }
