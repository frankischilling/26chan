// The owner supervisor supplies isolated processing, never approval fixtures.
// This process receives public HTTP authority and the receipts for its own PNGs.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chromium, expect } from '@playwright/test';
import { createInterface } from 'node:readline';
import { assertOwnedThreadResponse, ownedUploadResponse } from './owned-upload-response.mjs';
import { canvasProof, closePainter, decodedPngProof, drawingControls, pointerDrawing, pointerDrawingEdit, seedDrawingPng, tegakiModule, observeDrawingResponse, exportDrawingProof, observeDrawingCancellation, reportDrawingEditFailure } from './helpers/drawing-browser.mjs';

const [rawOrigin, board, marker, mode] = process.argv.slice(2), origin = new URL(rawOrigin);
assert.equal(process.argv.length, 6); assert.equal(origin.hostname, '127.0.0.1'); assert.equal(origin.protocol, 'http:');
assert.ok(mode === 'image-edit' ? board === 'i' : /^u[0-9a-f]{8}$/.test(board));
assert.match(marker, /^[a-f0-9]{32}$/); assert.ok(['ordinary', 'quick-reply', 'image-edit'].includes(mode));
for (const name of Object.keys(process.env)) assert.ok(!name.endsWith('DATABASE_URL') && !['POSTER_ID_KEY', 'PUBLIC_INTAKE_TOKEN'].includes(name), 'browser cannot inherit service credentials');
const url = path => new URL(path, origin).href, password = 'owned-drawing-password';
const replies = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
async function signal(kind, id, extra = '') {
  console.log(`DRAWING_${kind} ${marker} ${id}${extra ? ` ${extra}` : ''}`);
  const reply = await replies.next();
  assert.equal(reply.done, false); assert.equal(reply.value, `ACK ${marker} ${id}`);
}
let browser;
const stop = () => { if (browser) void browser.close(); };
process.once('SIGTERM', stop); process.once('SIGINT', stop);
try {
  browser = await chromium.launch({ timeout: 15000 });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage(); page.setDefaultTimeout(10000);
  const requests = [], violations = [];
  const cancellation = observeDrawingCancellation(page, url(`/${board}/upload/cancel`));
  page.on('request', request => requests.push({ url: request.url(), method: request.method() }));
  await context.exposeBinding('recordDrawingCsp', (_source, value) => violations.push(value));
  await context.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true, keyBinds: true, threadWatcher: true, autoUpdate: false }));
    document.addEventListener('securitypolicyviolation', event => { void window.recordDrawingCsp({ directive: event.effectiveDirective, uri: event.blockedURI }); });
  });
  let target = '0', ownerThread;
  {
    const created = await context.request.post(url(`/${board}/post`), { headers: { Origin: origin.origin }, maxRedirects: 0,
      form: { com: `Drawing ownership ${marker}`, password } });
    assertOwnedThreadResponse(created); ownerThread = /#p(\d+)$/.exec(created.headers().location)[1];
    target = mode === 'ordinary' ? '0' : ownerThread;
  }
  if (mode === 'image-edit') await signal('OWNER', ownerThread);
  const location = url(target === '0' ? `/${board}/` : `/${board}/thread/${target}`);
  let form, controls;
  const emitReceipt = async () => {
    const captured = await observeDrawingResponse(page, url(`/${board}/upload`), 'upload');
    await page.locator('#tegaki-finish-btn').click();
    const result = await captured(); assert.equal(result.status, 200); const receipt = result.result;
    assert.equal(receipt.resto, target); assert.equal(receipt.state, 'queued');
    assert.match(receipt.upload_id, /^[a-f0-9]{32}$/); assert.match(receipt.upload_capability, /^[a-f0-9]{64}$/);
    // The private pipe passes the receipt hash, never the usable capability.
    await signal('RECEIPT', receipt.upload_id, `${createHash('sha256').update(receipt.upload_capability).digest('hex')} ${target} ${ownerThread}`);
    return receipt;
  };
  const waitForReceipt = async receipt => {
    const deadline = Date.now() + 60000;
    while (await form.locator('[name=upload_id]').count() === 0) {
      assert.ok(Date.now() < deadline, 'isolated drawing approval deadline exceeded');
      const check = form.getByRole('button', { name: 'Check status', exact: true });
      if (await check.isVisible().catch(() => false) && await check.isEnabled()) await check.click();
      await new Promise(resolve => setTimeout(resolve, 500));
    }
    await expect(form.locator('[name=upload_id]')).toHaveValue(receipt.upload_id);
    await expect(form.locator('[name=upload_capability]')).toHaveValue(receipt.upload_capability);
  };
  if (mode === 'image-edit') {
    const ready = await page.goto(location);
    assert.equal(ready.status(), 200); assert.ok(ready.headers()['content-security-policy']);
    assert.doesNotMatch(ready.headers()['content-security-policy'], /unsafe-eval/);
    const ordinary = page.locator('form.postEditor').first();
    await expect(ordinary).toHaveAttribute('data-drawing-edit-allowed', 'true');
    await expect(ordinary).not.toHaveAttribute('data-drawing-allowed', 'true');
    await expect(ordinary.locator('[data-drawing-draw]')).toBeHidden();
    assert.equal(requests.some(request => request.url.endsWith(tegakiModule)), false, 'Edit-only Tegaki remains lazy');

    const seed = await seedDrawingPng(page);
    const response = await context.request.post(url(`/${board}/upload`), {
      headers: { Origin: origin.origin, Accept: 'application/json' },
      multipart: { resto: target, upfile: { name: 'tegaki.png', mimeType: 'image/png', buffer: seed.buffer } },
    });
    const received = await ownedUploadResponse(response, 'upload');
    assert.equal(received.status, 200); const firstReceipt = received.result;
    assert.equal(firstReceipt.resto, target); assert.equal(firstReceipt.state, 'queued');
    assert.match(firstReceipt.upload_id, /^[a-f0-9]{32}$/);
    assert.match(firstReceipt.upload_capability, /^[a-f0-9]{64}$/);
    await signal('RECEIPT', firstReceipt.upload_id,
      `${createHash('sha256').update(firstReceipt.upload_capability).digest('hex')} ${target} ${ownerThread}`);
    await signal('APPROVE', firstReceipt.upload_id);
    const deadline = Date.now() + 60000;
    for (;;) {
      assert.ok(Date.now() < deadline, 'seed image approval deadline exceeded');
      const checked = await context.request.post(url(`/${board}/upload/status`), {
        headers: { Origin: origin.origin, Accept: 'application/json' },
        form: { resto: target, upload_id: firstReceipt.upload_id, upload_capability: firstReceipt.upload_capability },
      });
      const result = await ownedUploadResponse(checked, 'upload');
      assert.equal(result.status, 200);
      assert.equal(result.result.resto, target); assert.equal(result.result.upload_id, firstReceipt.upload_id);
      assert.equal(result.result.upload_capability, firstReceipt.upload_capability);
      if (result.result.state === 'approved') break;
      assert.ok(['queued', 'processing'].includes(result.result.state), 'seed image approval failed');
      await new Promise(resolve => setTimeout(resolve, 500));
    }

    const seedPosted = await context.request.post(url(`/${board}/imgboard.php`), {
      headers: { Origin: origin.origin, Accept: 'application/json' },
      multipart: { resto: target, mode: 'regist', pwd: password, com: `Drawing qualification ${marker}`,
        upload_id: firstReceipt.upload_id, upload_capability: firstReceipt.upload_capability },
    });
    const seedResult = await ownedUploadResponse(seedPosted, 'post');
    assert.equal(seedResult.status, 200); assert.equal(String(seedResult.result.tid), target);
    assert.ok(Number.isSafeInteger(seedResult.result.pid) && seedResult.result.pid > Number(target));
    const seedPost = String(seedResult.result.pid), api = url(`/${board}/thread/${target}.json`);
    const originalPosts = (await (await context.request.get(api)).json()).posts;
    const sourcePost = originalPosts.find(value => String(value.no) === seedPost);
    assert.ok(sourcePost); assert.equal(sourcePost.ext, '.png');
    assert.equal(sourcePost.filename, 'tegaki'); assert.equal(sourcePost.w, 400); assert.equal(sourcePost.h, 400);
    assert.equal(sourcePost.com, `Drawing qualification ${marker}`);
    await page.goto(location);
    const seedFile = page.locator(`#f${seedPost}`);
    const media = await seedFile.locator('a[class="fileThumb"]').getAttribute('href');
    assert.equal(new URL(media).hostname, '127.0.0.1');
    assert.notEqual(new URL(media).origin, origin.origin);
    const normalized = await context.request.get(media); assert.equal(normalized.status(), 200);
    const normalizedProof = await decodedPngProof(page, await normalized.body());
    assert.equal(normalizedProof.width, 400); assert.equal(normalizedProof.height, 400);
    assert.equal(normalizedProof.hash, seed.proof.hash, 'isolated seed preserves its generated canvas pixels');

    const edit = page.locator(`#fT${seedPost} [data-drawing-edit]`);
    await expect(edit).toHaveText('Edit');
    await context.addCookies([{ name: 'drawing_cors_witness', value: 'owned-test-cookie',
      url: new URL(media).origin + '/', sameSite: 'Lax' }]);
    assert.ok((await context.cookies(media)).some(cookie => cookie.name === 'drawing_cors_witness'));
    const importedResponse = page.waitForResponse(value => value.url() === media
      && value.request().headers().origin === origin.origin);
    await edit.click();
    const sourceResponse = await importedResponse;
    assert.ok([200, 304].includes(sourceResponse.status()));
    const importedHeaders = await sourceResponse.allHeaders();
    assert.equal(importedHeaders['access-control-allow-origin'], origin.origin);
    assert.ok(importedHeaders.vary.split(',').some(value => value.trim().toLowerCase() === 'origin'));
    assert.equal((await sourceResponse.request().allHeaders()).cookie, undefined);
    await expect(page.locator('#tegaki-cursor-layer')).toBeVisible();
    assert.equal((await canvasProof(page)).hash, normalizedProof.hash, 'anonymous import preserves source pixels');
    const editedProof = await pointerDrawingEdit(page);
    await exportDrawingProof(page, editedProof);
    form = page.locator('#quickReply form'); controls = drawingControls(page.locator('#qr-painter-ctrl'));
    const editedReceipt = await emitReceipt();
    assert.notEqual(editedReceipt.upload_id, firstReceipt.upload_id);
    await signal('APPROVE', editedReceipt.upload_id);
    await waitForReceipt(editedReceipt);
    await expect(controls.draw).toHaveText('Edit');
    await expect(controls.width).toBeDisabled();
    await form.locator('[name=com]').fill(`Drawing edit qualification ${marker}`);
    const capturedPost = await observeDrawingResponse(page, url(`/${board}/imgboard.php`), 'post', { annotation: true });
    await form.locator('button[type=submit],input[type=submit]').click();
    const posted = await capturedPost();
    assert.equal(posted.status, 200); assert.equal(String(posted.result.tid), target);
    assert.ok(Number.isSafeInteger(posted.result.pid));
    assert.equal(posted.annotation?.oe_src, seedPost, 'source ID belongs to the imported post');
    assert.match(posted.annotation?.oe_time, /^[1-9][0-9]*$/, 'real editing elapsed at least one rounded second');
    assert.ok(Number.isSafeInteger(Number(posted.annotation.oe_time)) && Number(posted.annotation.oe_time) <= 5_184_000);
    const editedPost = String(posted.result.pid); assert.notEqual(editedPost, seedPost);
    await expect(form.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
    await expect(controls.draw).toBeHidden();

    const updatedFile = page.locator(`#f${editedPost}`);
    await expect(updatedFile.locator('[data-drawing-edit]')).toHaveText('Edit');
    const editedMedia = await updatedFile.locator('a[class="fileThumb"]').getAttribute('href');
    assert.notEqual(editedMedia, media); assert.equal(new URL(editedMedia).origin, new URL(media).origin);
    const editedPosts = (await (await context.request.get(api)).json()).posts;
    assert.equal(editedPosts.length, originalPosts.length + 1);
    const editedAttachment = editedPosts.find(value => String(value.no) === editedPost);
    assert.ok(editedAttachment); assert.equal(editedAttachment.ext, '.png');
    assert.equal(editedAttachment.filename, 'tegaki'); assert.equal(editedAttachment.w, 400); assert.equal(editedAttachment.h, 400);
    assert.ok(editedAttachment.com.includes(`Drawing edit qualification ${marker}`));
    const annotation = await page.locator(`#m${editedPost}`).innerHTML();
    assert.ok(annotation.includes('<b>Oekaki Post</b>'), 'published drawing has a rendered annotation');
    assert.ok(annotation.includes(`Source: &gt;&gt;${seedPost}`), 'source reference is HTML escaped');
    const commentText = await page.locator(`#m${editedPost}`).textContent();
    assert.ok(commentText.includes(`Drawing edit qualification ${marker}`));
    assert.ok(commentText.includes(`Source: >>${seedPost}`));
    const editedImage = await context.request.get(editedMedia); assert.equal(editedImage.status(), 200);
    assert.equal((await decodedPngProof(page, await editedImage.body())).hash, editedProof.hash,
      'isolated second approval publishes actual trusted Tegaki stroke pixels');

    const reused = await context.request.post(url(`/${board}/imgboard.php`), {
      headers: { Origin: origin.origin, Accept: 'application/json' },
      multipart: { resto: target, mode: 'regist', pwd: password, com: '',
        upload_id: editedReceipt.upload_id, upload_capability: editedReceipt.upload_capability },
    });
    assert.equal(typeof (await reused.json()).error, 'string');
    assert.equal((await (await context.request.get(api)).json()).posts.length, editedPosts.length);
    for (const [id, asset] of [[editedPost, editedMedia], [seedPost, media]]) {
      const deleted = await context.request.post(url(`/${board}/delete`), {
        headers: { Origin: origin.origin }, maxRedirects: 0,
        form: { no: id, password: '', file_only: 'true' },
      });
      assert.equal(deleted.status(), 303);
      const missing = await context.request.get(asset, { headers: { Origin: origin.origin } });
      assert.equal(missing.status(), 404); assert.equal(missing.headers()['access-control-allow-origin'], undefined);
      assert.equal((await (await context.request.get(api)).json()).posts.find(value => String(value.no) === id).filedeleted, 1);
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(ordinary.locator('[data-drawing-draw]')).toBeHidden();
    assert.deepEqual(violations, []);
    assert.equal(requests.some(request => /\.tgkr(?:$|\?)/.test(request.url)), false);
    console.log('PASS drawing image-edit: real Tegaki pointer strokes, isolated seed receipt, anonymous CORS import, source annotation, updater, one-use posting and owned deletion');
  } else {
  const response = await page.goto(location);
  assert.equal(response.status(), 200); assert.ok(response.headers()['content-security-policy']);
  assert.doesNotMatch(response.headers()['content-security-policy'], /unsafe-eval/);
  const ordinary = page.locator('form.postEditor').first();
  await expect(ordinary).toHaveAttribute('data-drawing-allowed', 'true');
  assert.equal(requests.some(request => request.url.endsWith(tegakiModule)), false, 'editor stays lazy before Draw');
  if (mode === 'quick-reply') { await page.locator('.open-qr-link').click(); form = page.locator('#quickReply form'); controls = drawingControls(page.locator('#qr-painter-ctrl')); }
  else {
    const reveal = page.locator('#togglePostFormLink a'); if (await reveal.isVisible()) await reveal.click();
    form = ordinary; controls = drawingControls(ordinary);
  }
  await expect(controls.width).toHaveValue('400'); await expect(controls.height).toHaveValue('400');
  await expect(controls.clear).toBeDisabled(); await expect(form.locator('.oe-r-cb')).toHaveCount(0);
  await controls.draw.click(); const firstProof = await pointerDrawing(page);
  await expect(page.locator('[data-drawing-import-unavailable]')).toHaveText('Open (unavailable)');
  await expect(page.locator('[data-drawing-import-unavailable]')).toHaveAttribute('aria-disabled', 'true');
  await expect(page.locator('#tegaki-filepicker')).toBeDisabled();
  await page.keyboard.press('w');
  if (target !== '0') await expect(page.locator(`#watch-${target}-${board}`)).toHaveCount(0);
  await closePainter(page, false); assert.equal((await canvasProof(page)).hash, firstProof.hash);
  const firstReceipt = await emitReceipt();
  await expect(controls.draw).toHaveText('Edit'); await expect(controls.width).toBeDisabled();
  await expect(form.locator('[name=upload_id]')).toHaveCount(0); // queued is not approved
  await controls.draw.click();
  // The retained hidden canvas is readable while Edit is still canceling.
  // Reopening the painter is the completion boundary: prepare() must first
  // successfully revoke the previous receipt. A failed cancel never opens it.
  await expect(page.locator('#tegaki-cursor-layer')).toBeVisible().catch(async error => {
    await reportDrawingEditFailure(page, controls, cancellation); throw error;
  });
  assert.equal((await canvasProof(page)).hash, firstProof.hash);
  await signal('REVOKED', firstReceipt.upload_id);
  // Edit cancels the old approval without destroying the retained canvas.
  await closePainter(page, true); await expect(page.locator('#tegaki')).toHaveCount(0);
  await expect(controls.draw).toHaveText('Draw'); await expect(controls.width).toBeEnabled();
  await controls.draw.click(); await pointerDrawing(page); const clearReceipt = await emitReceipt();
  let dialogs = 0; const countDialog = async dialog => { dialogs++; await dialog.dismiss(); }; page.on('dialog', countDialog);
  await controls.clear.click(); await expect(controls.draw).toHaveText('Draw'); page.off('dialog', countDialog); await signal('REVOKED', clearReceipt.upload_id);
  assert.equal(dialogs, 0, 'form Clear is separate from the destructive editor confirmation');
  await controls.draw.click(); assert.ok((await canvasProof(page)).ink > 100, 'form Clear retains the source canvas');
  await closePainter(page, true);
  if (mode === 'quick-reply') {
    await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
    await page.locator('.open-qr-link').click(); form = page.locator('#quickReply form'); controls = drawingControls(page.locator('#qr-painter-ctrl'));
    await expect(form.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
    await expect(controls.draw).toHaveText('Draw');
  }
  await controls.draw.click(); const finalProof = await pointerDrawing(page); await exportDrawingProof(page, finalProof); const receipt = await emitReceipt();
  await signal('APPROVE', receipt.upload_id);
  await waitForReceipt(receipt);
  await form.locator('[name=com]').fill(`Drawing qualification ${marker}`);
  const posted = await observeDrawingResponse(page, url(`/${board}/imgboard.php`), 'post');
  await form.locator('button[type=submit],input[type=submit]').click();
  const result = await posted(); assert.equal(result.status, 200); assert.ok(Number.isSafeInteger(result.result.tid)); assert.ok(Number.isSafeInteger(result.result.pid)); assert.equal(String(result.result.tid), target);
  const post = String(result.result.pid), thread = target === '0' ? post : target;
  if (mode === 'ordinary') await expect(page).toHaveURL(url(`/${board}/thread/${thread}#p${post}`));
  else {
    await expect(form.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
    await expect(controls.draw).toHaveText('Draw');
  }
  const api = url(`/${board}/thread/${thread}.json`), posts = (await (await context.request.get(api)).json()).posts;
  const attached = posts.find(value => String(value.no) === post);
  assert.equal(attached.ext, '.png'); assert.equal(attached.w, 400); assert.equal(attached.h, 400);
  assert.equal(attached.filename, 'tegaki'); assert.equal(attached.com, `Drawing qualification ${marker}`);
  const replay = await context.request.post(url(`/${board}/imgboard.php`), { headers: { Origin: origin.origin, Accept: 'application/json' },
    multipart: { resto: thread, mode: 'regist', pwd: password, com: '', upload_id: receipt.upload_id, upload_capability: receipt.upload_capability } });
  assert.equal(typeof (await replay.json()).error, 'string');
  assert.equal((await (await context.request.get(api)).json()).posts.length, posts.length);
  await page.goto(url(`/${board}/thread/${thread}`));
  const file = page.locator(`#p${post} .file`), media = await file.locator('a.fileThumb').getAttribute('href');
  assert.equal(new URL(media).hostname, '127.0.0.1'); assert.notEqual(new URL(media).origin, origin.origin);
  const normalized = await context.request.get(media); assert.equal(normalized.status(), 200);
  const decoded = await decodedPngProof(page, await normalized.body());
  assert.equal(decoded.width, 400); assert.equal(decoded.height, 400); assert.ok(decoded.ink > 100);
  assert.equal(decoded.hash, finalProof.hash, 'isolated normalized attachment retains the actual pointer-drawn pixels');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('form.postEditor [data-drawing-draw]')).toBeHidden();
  // Verify owned anonymous session deletion without sending a password again.
  const deleted = await context.request.post(url(`/${board}/delete`), { headers: { Origin: origin.origin }, maxRedirects: 0,
    form: { no: post, password: '', file_only: 'true' } });
  assert.equal(deleted.status(), 303); assert.equal((await context.request.get(media)).status(), 404);
  assert.equal((await (await context.request.get(api)).json()).posts.find(value => String(value.no) === post).filedeleted, 1);
  assert.deepEqual(violations, []);
  assert.equal(requests.some(request => /\.tgkr(?:$|\?)/.test(request.url)), false);
  console.log(`PASS drawing ${mode}: real Tegaki pointer strokes, retained Edit/Clear, confirmed Cancel, isolated receipt-bound approval, nonblank normalized PNG, one-use posting and owned deletion`);
  }
} finally { if (browser) await browser.close(); await replies.return?.(); }
