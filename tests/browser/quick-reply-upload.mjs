// The supervisor supplies isolated processing. This browser has only public
// HTTP authority and the one-use approval issued to its own upload.
import assert from 'node:assert/strict';
import { chromium, expect } from '@playwright/test';

const origin = new URL(process.argv[2]), board = process.argv[3], source = process.argv[4];
assert.equal(process.argv.length, 5); assert.equal(origin.hostname, '127.0.0.1'); assert.equal(origin.protocol, 'http:');
assert.match(board, /^[a-z0-9]{1,10}$/);
const password = 'owned-quick-reply-upload-password';
let browser;
const stop = () => { if (browser) void browser.close(); };
process.once('SIGTERM', stop); process.once('SIGINT', stop);
try {
  browser = await chromium.launch({ timeout: 15000 });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage(); page.setDefaultTimeout(10000);
  const url = path => new URL(path, origin).href;
  const created = await context.request.post(url(`/${board}/post`), { headers: { Origin: origin.origin }, maxRedirects: 0,
    form: { com: 'Owned image Quick Reply thread', password } });
  assert.equal(created.status(), 303); const thread = /#p(\d+)$/.exec(created.headers().location)[1];
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
  await page.goto(url(`/${board}/thread/${thread}`));
  await page.getByLabel('File', { exact: true }).setInputFiles(source);
  await page.getByRole('button', { name: 'Upload file', exact: true }).click();
  const deadline = Date.now() + 45000;
  while (await page.getByRole('button', { name: 'Check upload status', exact: true }).count()) {
    assert.ok(Date.now() < deadline, 'isolated approval deadline exceeded');
    await page.getByRole('button', { name: 'Check upload status', exact: true }).click();
    if (await page.locator('form.postEditor').count()) break;
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  await expect(page.locator('form.postEditor')).toBeVisible();
  const approvalUrl = page.url(), native = page.locator('form.postEditor').first();
  const uploadId = await native.locator('[name=upload_id]').inputValue();
  const capability = await native.locator('[name=upload_capability]').inputValue();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  const qr = page.locator('#quickReply');
  await expect(qr.locator('[name=upload_id]')).toHaveValue(uploadId);
  await expect(qr.locator('[name=upload_capability]')).toHaveValue(capability);
  await qr.locator('[name=spoiler]').check(); await page.locator('#qr-pwd').fill(password);
  const posted = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === url(`/${board}/imgboard.php`));
  await qr.locator('input[type=submit]').click(); const response = await posted;
  assert.equal(response.status(), 200); const result = await response.json();
  assert.equal(String(result.tid), thread); assert.ok(result.pid > result.tid);
  const post = String(result.pid); await expect(qr).toBeVisible(); await expect(page.locator('#qrCom')).toHaveValue('');
  assert.equal(page.url(), approvalUrl);
  await expect(page.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(2); // Cancel form retains revocation fields only.
  await expect(qr.locator('[name=spoiler], [name=upload_id]')).toHaveCount(0);
  await expect(native.locator('[name=upload_id], [name=spoiler]')).toHaveCount(0);
  await expect(native.locator('[name=com]')).toHaveAttribute('required', '');
  await expect.poll(() => page.evaluate(({ board, thread, post }) => JSON.parse(localStorage.getItem(`4chan-track-${board}-${thread}`) || '{}')[`>>${post}`], { board, thread, post })).toBe(1);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  await expect(qr.locator('[name=upload_id], [name=upload_capability], [name=spoiler]')).toHaveCount(0);
  await page.locator('#qr-pwd').fill(password); await page.locator('#qrCom').fill('Text after the consumed image');
  const next = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === url(`/${board}/imgboard.php`));
  await qr.locator('input[type=submit]').click(); assert.equal((await next).status(), 200);
  await expect(page.locator('#qrCom')).toHaveValue('');
  const api = url(`/${board}/thread/${thread}.json`);
  const posts = (await (await context.request.get(api)).json()).posts;
  assert.equal(posts.length, 3); const attached = posts.find(value => String(value.no) === post);
  assert.equal(attached.spoiler, 1); assert.equal(attached.com, undefined); assert.equal(attached.ext, '.png');
  assert.equal(posts[2].com, 'Text after the consumed image'); assert.equal(posts[2].tim, undefined);
  const replay = await context.request.post(url(`/${board}/imgboard.php`), { headers: { Origin: origin.origin, Accept: 'application/json' },
    multipart: { resto: thread, mode: 'regist', pwd: password, com: '', upload_id: uploadId, upload_capability: capability } });
  assert.equal(typeof (await replay.json()).error, 'string');
  assert.equal((await (await context.request.get(api)).json()).posts.length, 3);
  await page.goto(url(`/${board}/thread/${thread}`));
  await expect(page.locator(`#p${post} .file img`)).toHaveCount(0);
  await page.locator(`#p${post} .file details summary`).click();
  const link = page.locator(`#p${post} .file details a`), media = await link.getAttribute('href');
  assert.notEqual(new URL(media).origin, origin.origin); assert.equal((await context.request.get(media)).status(), 200);
  const opening = page.waitForEvent('popup'); await link.click(); const imagePage = await opening;
  await expect.poll(() => imagePage.locator('img').evaluate(image => image.naturalWidth)).toBe(1); await imagePage.close();
  const deletion = page.locator(`#p${post} form[action$="/delete"]`);
  await page.locator(`#p${post} .postActions summary`).click();
  await deletion.locator('[name=password]').fill(password); await deletion.locator('[name=file_only]').check();
  await deletion.getByRole('button', { name: 'Delete post', exact: true }).click();
  assert.equal((await context.request.get(media)).status(), 404);
  const deleted = (await (await context.request.get(api)).json()).posts.find(value => String(value.no) === post);
  assert.equal(deleted.filedeleted, 1); assert.equal(deleted.tim, undefined);
  console.log('PASS JavaScript upload, isolated approval, persisted posting through Quick Reply, consumed capabilities, text follow-up, replay denial and file-only deletion');
} finally { if (browser) await browser.close(); }
