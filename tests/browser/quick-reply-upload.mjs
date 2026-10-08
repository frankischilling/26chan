// The supervisor supplies isolated processing. This browser has only public
// HTTP authority and the one-use approval issued to its own upload.
import assert from 'node:assert/strict';
import { chromium, expect } from '@playwright/test';
import { observeOwnedDeletionResponse, observeOwnedUploadResponse } from './owned-upload-response.mjs';

const origin = new URL(process.argv[2]), board = process.argv[3], source = process.argv[4], flags = process.argv.slice(5);
assert.ok(flags.every(flag => ['--inline', '--no-spoilers'].includes(flag))); assert.equal(new Set(flags).size, flags.length);
const inline = flags.includes('--inline'), spoilers = !flags.includes('--no-spoilers');
assert.equal(origin.hostname, '127.0.0.1'); assert.equal(origin.protocol, 'http:');
assert.match(board, /^u[0-9a-f]{8}$/);
// Private fixture identity and cleanup remain exclusively in the supervisor.
for (const name of Object.keys(process.env)) {
  assert.ok(!name.endsWith('DATABASE_URL') && !['POSTER_ID_KEY', 'PUBLIC_INTAKE_TOKEN'].includes(name),
    'browser qualification must not inherit service credentials');
}
const password = 'owned-quick-reply-upload-password';
let browser;
const stop = () => { if (browser) void browser.close(); };
process.once('SIGTERM', stop); process.once('SIGINT', stop);
try {
  browser = await chromium.launch({ timeout: 15000 });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage(); page.setDefaultTimeout(10000);
  const requests = []; page.on('request', request => requests.push(request.url()));
  const url = path => new URL(path, origin).href;
  const created = await context.request.post(url(`/${board}/post`), { headers: { Origin: origin.origin }, maxRedirects: 0,
    form: { com: 'Owned image Quick Reply thread', password } });
  assert.equal(created.status(), 303); const thread = /#p(\d+)$/.exec(created.headers().location)[1];
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
  await page.goto(url(`/${board}/thread/${thread}`));
  let approvalUrl, native, uploadId, capability, qr;
  const deadline = Date.now() + 45000;
  if (inline) {
    await page.locator('.open-qr-link').click();
    qr = page.locator('#quickReply');
    await expect(qr.locator('#qrFile')).toBeVisible();
    const queued = await observeOwnedUploadResponse(page, url(`/${board}/upload`), 'upload');
    await qr.locator('#qrFile').setInputFiles(source);
    const queuedResponse = await queued(); assert.equal(queuedResponse.status, 200);
    const receipt = queuedResponse.result;
    assert.equal(receipt.resto, thread); assert.equal(receipt.state, 'queued');
    uploadId = receipt.upload_id; capability = receipt.upload_capability;
    assert.match(uploadId, /^[0-9a-f]{32}$/); assert.match(capability, /^[0-9a-f]{64}$/);
    while (await qr.locator('[name=upload_id]').count() === 0) {
      assert.ok(Date.now() < deadline, 'inline isolated approval deadline exceeded');
      const check = qr.getByRole('button', { name: 'Check status', exact: true });
      if (await check.isVisible().catch(() => false)) await check.click();
      await new Promise(resolve => setTimeout(resolve, 500));
    }
    approvalUrl = page.url(); native = page.locator('form.postEditor').first();
  } else {
    await page.getByLabel('File', { exact: true }).setInputFiles(source);
    await page.getByRole('button', { name: 'Upload file', exact: true }).click();
    while (await page.getByRole('button', { name: 'Check upload status', exact: true }).count()) {
      assert.ok(Date.now() < deadline, 'isolated approval deadline exceeded');
      await page.getByRole('button', { name: 'Check upload status', exact: true }).click();
      if (await page.locator('form.postEditor').count()) break;
      await new Promise(resolve => setTimeout(resolve, 2000));
    }
    await expect(page.locator('form.postEditor')).toBeVisible();
    approvalUrl = page.url(); native = page.locator('form.postEditor').first();
    uploadId = await native.locator('[name=upload_id]').inputValue();
    capability = await native.locator('[name=upload_capability]').inputValue();
    await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
    qr = page.locator('#quickReply');
  }
  await expect(qr.locator('[name=upload_id]')).toHaveValue(uploadId);
  await expect(qr.locator('[name=upload_capability]')).toHaveValue(capability);
  if (spoilers) await qr.locator('[name=spoiler]').check();
  else {
    await expect(qr.locator('[name=spoiler]')).toHaveCount(0);
    await expect(native.locator('[name=spoiler]')).toHaveCount(0);
    // A forged choice must still be ignored by the locked server policy.
    await qr.locator('form').evaluate(form => { const row = document.createElement('div'); row.className = 'qr-approved-image'; const input = document.createElement('input'); input.type = 'hidden'; input.name = 'spoiler'; input.value = 'on'; row.append(input); form.append(row); });
  }
  await expect(page.locator('#qr-pwd')).toHaveValue('');
  const posted = await observeOwnedUploadResponse(page, url(`/${board}/imgboard.php`), 'post');
  await qr.locator('input[type=submit]').click(); const response = await posted();
  assert.equal(response.status, 200); const result = response.result;
  assert.equal(String(result.tid), thread); assert.ok(result.pid > result.tid);
  const post = String(result.pid); await expect(qr).toBeVisible(); await expect(page.locator('#qrCom')).toHaveValue('');
  assert.equal(page.url(), approvalUrl);
  await expect(page.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(inline ? 0 : 2); // HTML confirmation retains only its cancel fields.
  if (inline) {
    await expect(qr.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
    if (spoilers) await expect(qr.locator('[name=spoiler]')).toBeDisabled();
    else await expect(qr.locator('[name=spoiler]')).toHaveCount(0);
  } else {
    await expect(qr.locator('[name=spoiler], [name=upload_id]')).toHaveCount(0);
  }
  await expect(native.locator('[name=upload_id], [name=spoiler]')).toHaveCount(0);
  await expect(native.locator('[name=com]')).toHaveAttribute('required', '');
  await expect.poll(() => page.evaluate(({ board, thread, post }) => JSON.parse(localStorage.getItem(`4chan-track-${board}-${thread}`) || '{}')[`>>${post}`], { board, thread, post })).toBe(1);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  if (inline) await page.locator('.open-qr-link').click();
  else await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  await expect(qr.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
  if (inline && spoilers) await expect(qr.locator('[name=spoiler]')).toBeDisabled();
  else await expect(qr.locator('[name=spoiler]')).toHaveCount(0);
  await expect(page.locator('#qr-pwd')).toHaveValue(''); await page.locator('#qrCom').fill('Text after the consumed image');
  const next = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === url(`/${board}/imgboard.php`));
  await qr.locator('input[type=submit]').click(); assert.equal((await next).status(), 200);
  await expect(page.locator('#qrCom')).toHaveValue('');
  const api = url(`/${board}/thread/${thread}.json`);
  const posts = (await (await context.request.get(api)).json()).posts;
  assert.equal(posts.length, 3); const attached = posts.find(value => String(value.no) === post);
  assert.equal(attached.spoiler, spoilers ? 1 : undefined); assert.equal(attached.com, undefined); assert.equal(attached.ext, '.png');
  assert.equal(posts[2].com, 'Text after the consumed image'); assert.equal(posts[2].tim, undefined);
  const replay = await context.request.post(url(`/${board}/imgboard.php`), { headers: { Origin: origin.origin, Accept: 'application/json' },
    multipart: { resto: thread, mode: 'regist', pwd: password, com: '', upload_id: uploadId, upload_capability: capability } });
  assert.equal(typeof (await replay.json()).error, 'string');
  assert.equal((await (await context.request.get(api)).json()).posts.length, 3);
  await page.goto(url(`/${board}/thread/${thread}`));
  const file = page.locator(`#p${post} .file`);
  if (spoilers) {
    await expect(file.locator('a.fileThumb.imgspoiler img')).toHaveAttribute('src', '/static/catalog/spoiler.png');
    await expect(file.locator('.fileText > a')).toHaveText('Spoiler Image');
  } else await expect(file.locator('a.fileThumb.imgspoiler')).toHaveCount(0);
  const link = file.locator('a.fileThumb'), media = await link.getAttribute('href');
  await expect(file.locator('img')).toHaveCount(1);
  await expect(file.locator('img')).toHaveAttribute('width', spoilers ? '100' : '1');
  await expect(file.locator('img')).toHaveAttribute('height', spoilers ? '100' : '1');
  assert.equal(requests.includes(media), false, 'thumbnail rendering must not fetch the original file');
  assert.equal(requests.includes(media.replace(/\.png$/, 's.jpg')), !spoilers, 'only an unspoiled file fetches its approved thumbnail');
  assert.notEqual(new URL(media).origin, origin.origin); assert.equal((await context.request.get(media)).status(), 200);
  const opening = page.waitForEvent('popup'); await (spoilers ? link : file.locator('.fileText > a')).click(); const imagePage = await opening;
  await expect.poll(() => imagePage.locator('img').evaluate(image => image.naturalWidth)).toBe(1); await imagePage.close();
  const deletion = page.locator(`#p${post} form[action$="/delete"]`);
  if (inline) {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.evaluate(post => {
      localStorage.removeItem('4chan_never_show_mobile');
      window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile', storageArea: localStorage }));
      window.ownedMobileFileDeletion = {
        document, post: document.getElementById(`p${post}`), file: document.getElementById(`f${post}`),
        image: document.querySelector(`#f${post} .fileThumb > img`),
        text: document.getElementById(`m${post}`).textContent, history: history.length,
      };
    }, post);
    const currentUrl = page.url(), endpoint = url(`/${board}/imgboard.php`), deletionRequests = [];
    page.on('request', request => {
      if (request.method() === 'POST' && [endpoint, url(`/${board}/delete`)].includes(request.url())) deletionRequests.push(request);
    });
    const menu = page.getByRole('button', { name: `Post menu for post ${post}`, exact: true });
    await expect(menu).toHaveText('...');
    await menu.click();
    await expect(page.getByRole('menuitem', { name: 'Delete file', exact: true })).toBeVisible();
    const finished = await observeOwnedDeletionResponse(page, endpoint);
    const confirmation = page.waitForEvent('dialog');
    const clicked = page.getByRole('menuitem', { name: 'Delete file', exact: true }).click();
    const dialog = await confirmation, message = dialog.message(), type = dialog.type();
    await dialog.accept(); await clicked;
    assert.equal(type, 'confirm'); assert.equal(message, 'Delete file?');
    const removed = await finished();
    assert.equal(removed.status, 200);
    assert.ok(removed.text.includes('The deletion was completed.'));
    await expect(page.locator('.nativeDeletionFeedback')).toHaveText(`Post No.${post}: File deleted.`);
    await expect(file).toHaveClass(/\bdeleted\b/);
    await expect(file.locator('.fileThumb > img')).toHaveClass(/\bdeleted\b/);
    await expect(file.locator('.fileThumb > img')).toHaveCSS('opacity', '0.66');
    await expect(file).toHaveCSS('opacity', '1');
    await expect(page.locator(`#pc${post}`)).not.toHaveClass(/\bdeleted\b/);
    await expect(page.locator(`#p${post}`)).not.toHaveAttribute('aria-busy');
    await expect(deletion.locator('[name=file_only]')).not.toBeChecked();
    await expect(deletion.locator('[name=password]')).toHaveValue('');
    assert.equal(page.url(), currentUrl);
    assert.equal(await page.evaluate(post => {
      const original = window.ownedMobileFileDeletion;
      return original.document === document && original.history === history.length
        && original.post === document.getElementById(`p${post}`)
        && original.file === document.getElementById(`f${post}`)
        && original.image === document.querySelector(`#f${post} .fileThumb > img`)
        && original.text === document.getElementById(`m${post}`).textContent;
    }, post), true);
    assert.equal(deletionRequests.length, 1);
    assert.equal(deletionRequests[0].url(), endpoint);
    assert.equal(deletionRequests[0].isNavigationRequest(), false);
    assert.deepEqual([...new URLSearchParams(deletionRequests[0].postData())].sort(),
      [[post, 'delete'], ['mode', 'usrdel'], ['onlyimgdel', 'on']].sort());
    await menu.click();
    await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toBeVisible();
    for (const name of ['Delete file', 'Open normalized file', 'Image search',
      'Search image on Google', 'Search image on Yandex', 'Search image on SauceNAO']) {
      await expect(page.getByRole('menuitem', { name, exact: true })).toHaveCount(0);
    }
    await page.keyboard.press('Escape');
    // Exercise real clicks on retained links. Deletion must prevent both native
    // expansion and ordinary navigation rather than merely hiding menu entries.
    await page.evaluate(post => {
      window.ownedDeletedFileClicks = [];
      const record = event => {
        if (event.target.closest(`#f${post} a[href]`)) window.ownedDeletedFileClicks.push(event.defaultPrevented);
      };
      document.addEventListener('click', record); document.addEventListener('auxclick', record);
    }, post);
    const mediaRequests = requests.filter(value => value === media || value === media.replace(/\.png$/, 's.jpg')).length;
    const openPages = context.pages().length;
    await link.click(); await link.click({ button: 'middle' });
    // The source hides desktop file labels at mobile widths. Exercise that
    // retained link where it is actually visible instead of forcing a click.
    await page.setViewportSize({ width: 1280, height: 900 });
    await expect(file.locator('.fileText > a')).toBeVisible();
    await file.locator('.fileText > a').click();
    assert.deepEqual(await page.evaluate(() => window.ownedDeletedFileClicks), [true, true, true]);
    await expect(file.locator('.expanded-thumb')).toHaveCount(0);
    await expect(page.locator('#image-hover')).toHaveCount(0);
    assert.equal(context.pages().length, openPages);
    assert.equal(requests.filter(value => value === media || value === media.replace(/\.png$/, 's.jpg')).length, mediaRequests);
    assert.equal(page.url(), currentUrl); assert.equal(deletionRequests.length, 1);
    const after = await context.request.get(api); assert.equal(after.status(), 200);
    const retained = (await after.json()).posts;
    assert.deepEqual(retained.map(value => String(value.no)), posts.map(value => String(value.no)));
    assert.equal(retained.find(value => String(value.no) === post).com, attached.com);
    assert.equal(retained.find(value => String(value.no) === post).filedeleted, 1);
    assert.equal(retained[2].com, 'Text after the consumed image');
    assert.equal((await context.request.get(media)).status(), 404);
    await page.reload();
  } else {
    // Keep the ordinary server form/redirect covered separately from the native
    // mobile action; it uses the same real anonymous session proof.
    await page.locator(`#p${post} .postActions summary`).click();
    await expect(deletion.locator('[name=password]')).toHaveValue(''); await deletion.locator('[name=file_only]').check();
    await deletion.getByRole('button', { name: 'Delete post', exact: true }).click();
  }
  await expect(page.locator(`#p${post} .file img.fileDeletedRes`)).toHaveAttribute('src', '/static/catalog/filedeleted-res.gif');
  await expect(page.locator(`#p${post} .file a`)).toHaveCount(0);
  assert.equal((await context.request.get(media)).status(), 404);
  const deleted = (await (await context.request.get(api)).json()).posts.find(value => String(value.no) === post);
  assert.equal(deleted.filedeleted, 1); assert.equal(deleted.tim, undefined); assert.equal(deleted.spoiler, spoilers ? 1 : undefined);
  console.log(`PASS JavaScript upload, isolated approval, persisted posting through Quick Reply, ${inline ? 'inline selection, ' : ''}consumed capabilities, text follow-up, replay denial and ${inline ? 'mobile in-place file-only deletion' : 'file-only form deletion'}`);
} finally { if (browser) await browser.close(); }
