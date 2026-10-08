import { test, expect } from '../helpers/visual-diagnostics.js';
test.use({ javaScriptEnabled: true });

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} Quick Reply preserves quote editing and fits desktop/mobile`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/demo/');
      const link = page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first(); const id = (await link.textContent());
      await link.click();
      const dialog = page.locator('#quickReply'); await expect(dialog).toBeVisible();
      await expect(page.locator('#qrCom')).toHaveValue(`>>${id}\n`);
      await expect(page.locator('#qrCom')).toBeFocused();
      await page.locator('#qrCom').fill('Selected fold'); await page.locator('#qrCom').selectText();
      await page.locator('#qrCom').press('Control+s');
      await expect(page.locator('#qrCom')).toHaveValue('[spoiler]Selected fold[/spoiler]');
      await expect(page.locator('#qr-pwd')).toHaveValue('');
      const bounds = await dialog.boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0); expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
      expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
      const path = info.outputPath(`${theme}-${width}-quick-reply.png`);
      await dialog.screenshot({ path, animations: 'disabled' });
      await info.attach(`${theme} ${width} Quick Reply`, { path, contentType: 'image/png' });
      await page.locator('#qrCom').press('Escape'); await expect(dialog).toHaveCount(0);
      await expect(page.locator('#com')).toHaveValue('');
    }
  });
}

test('Quick Reply drag uses bounded coordinates and thread navigation exposes the source entry', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  await page.locator('.open-qr-link').click();
  const header = page.locator('#qrHeader'), before = await header.boundingBox();
  await page.mouse.move(before.x + 30, before.y + 8); await page.mouse.down();
  await page.mouse.move(60, 65); await page.mouse.up();
  const after = await page.locator('#quickReply').boundingBox();
  expect(after.x).toBeGreaterThanOrEqual(0); expect(after.y).toBeGreaterThanOrEqual(0);
  expect(after.x).toBeLessThan(before.x);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.locator('.open-qr-link').click();
  const restored = await page.locator('#quickReply').boundingBox(); expect(restored.x).toBe(after.x); expect(restored.y).toBe(after.y);
});

test('Quick Reply renders errors as text, aborts without retry and ignores late completions after close', async ({ page }) => {
  await page.goto('/demo/'); await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click();
  await page.locator('#qrCom').fill('Owned draft'); await expect(page.locator('#qr-pwd')).toHaveValue('');
  let calls = 0;
  await page.route('**/demo/imgboard.php', async route => {
    calls++; await route.fulfill({ contentType: 'application/json', body: JSON.stringify({ error: '<img src=x onerror=alert(1)> rejected' }) });
  });
  const submit = page.locator('#quickReply input[type=submit]'); await submit.click();
  await expect(page.locator('#qrError')).toHaveText('<img src=x onerror=alert(1)> rejected');
  await expect(page.locator('#qrError img')).toHaveCount(0); await expect(page.locator('#qrCom')).toHaveValue('Owned draft');
  await page.unroute('**/demo/imgboard.php');
  let held;
  await page.route('**/demo/imgboard.php', route => { calls++; held = route; });
  await submit.click(); await expect(submit).toHaveValue('Sending');
  await expect.poll(() => calls).toBe(2);
  await submit.click(); await expect(submit).toHaveValue('Post');
  await expect(page.locator('#qrError')).toContainText('Check the thread');
  expect(calls).toBe(2); await expect(page.locator('#qrCom')).toHaveValue('Owned draft');
  await held.fulfill({ contentType: 'application/json', body: '{"tid":1000001,"pid":1000010}' }).catch(() => {});
  await submit.click(); await expect(submit).toHaveValue('Sending');
  await expect.poll(() => calls).toBe(3);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await held.fulfill({ contentType: 'application/json', body: '{"tid":1000001,"pid":1000011}' }).catch(() => {});
  await expect(page.locator('#quickReply')).toHaveCount(0); expect(calls).toBe(3);
  await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click();
  await expect(page.locator('#qrCom')).toHaveValue('>>1000001\n');
  await expect(submit).toHaveValue('Post'); await expect(page.locator('#com')).toHaveValue('');
});

test('Quick Reply guards closed threads and keeps the source spoiler caret behavior', async ({ page }) => {
  await page.goto('/demo/'); await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click();
  const comment = page.locator('#qrCom');
  await comment.fill(''); await comment.press('Control+s');
  expect(await comment.evaluate(node => node.selectionStart)).toBe(9);
  await comment.fill('suffix'); await comment.evaluate(node => node.setSelectionRange(0, 0)); await comment.press('Control+s');
  await expect(comment).toHaveValue('[spoiler][/spoiler]suffix'); expect(await comment.evaluate(node => node.selectionStart)).toBe(19);
  await page.evaluate(() => { document.querySelector('.thread').dataset.closed = 'true'; document.dispatchEvent(new Event('boardThreadStateChanged')); });
  await expect(page.locator('#quickReply input[type=submit]')).toBeDisabled(); await expect(page.locator('#qrError')).toHaveText('This thread is closed.');
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  const before = page.url(), warning = page.waitForEvent('dialog').then(async dialog => {
    expect(dialog.type()).toBe('alert'); expect(dialog.message()).toBe('This thread is closed'); await dialog.accept();
  });
  await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click(); await warning;
  await expect(page.locator('#quickReply')).toHaveCount(0); expect(page.url()).toBe(before);
});

test('media-enabled Quick Reply exposes one visible inline file selector', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  await page.locator('.open-qr-link').click();
  const qr = page.locator('#quickReply');
  await expect(qr.locator('#qrFile')).toBeVisible();
  await expect(qr.locator('#qrFile')).toHaveAttribute('type', 'file');
  await expect(qr.locator('#qrFile')).toHaveAttribute('accept', 'image/png,image/jpeg,image/gif');
  await expect(qr.locator('#qrFile')).toHaveAttribute('title', /replace|remove/i);
  await expect(qr.locator('.qr-upload-link')).toHaveCount(0);
});

test('inline Quick Reply upload reaches approval through bounded status checks and submits only the approved capability', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  const calls = [];
  const receipt = { upload_id: '1'.repeat(32), upload_capability: '2'.repeat(64), resto: '1000201' };
  let statuses = 0;
  await page.route('**/img/upload', async route => {
    calls.push('upload');
    const request = route.request();
    expect(request.method()).toBe('POST');
    expect(request.headers().accept).toBe('application/json');
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'queued' }) });
  });
  await page.route('**/img/upload/status', async route => {
    calls.push('status');
    statuses++;
    await route.fulfill({ contentType: 'application/json',
      body: JSON.stringify({ ...receipt, state: statuses < 2 ? 'processing' : 'approved' }) });
  });
  let posted;
  await page.route('**/img/imgboard.php', async route => {
    posted = await route.request().postDataBuffer();
    await route.fulfill({ contentType: 'application/json', body: '{"tid":1000201,"pid":1000207}' });
  });
  await page.locator('.open-qr-link').click();
  const qr = page.locator('#quickReply');
  await qr.locator('#qrFile').setInputFiles({ name: 'inline.png', mimeType: 'image/png', buffer: Buffer.from([1, 2, 3]) });
  await expect(qr.locator('#qrUploadStatus')).toContainText('queued');
  await expect(qr.locator('input[type=submit]')).toBeDisabled();
  await expect.poll(() => statuses, { timeout: 8_000 }).toBe(2);
  await expect(qr.locator('#qrUploadStatus')).toContainText('approved');
  await expect(qr.locator('[name=upload_id]')).toHaveValue(receipt.upload_id);
  await expect(qr.locator('[name=upload_capability]')).toHaveValue(receipt.upload_capability);
  await expect(qr.locator('[name=spoiler]')).toBeEnabled();
  await qr.locator('[name=spoiler]').check();
  await expect(qr.locator('#qr-pwd')).toHaveValue('');
  await qr.locator('input[type=submit]').click();
  await expect.poll(() => posted !== undefined).toBe(true);
  const text = posted.toString('utf8');
  expect(text).toContain(receipt.upload_id);
  expect(text).toContain(receipt.upload_capability);
  expect(calls).toEqual(['upload', 'status', 'status']);
});

test('inline file replacement cancels the known receipt first and a failed cancel keeps the old approval', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  const first = { upload_id: '1'.repeat(32), upload_capability: '2'.repeat(64), resto: '1000201', state: 'queued' };
  let uploads = 0, cancels = 0, denyCancel = true;
  await page.route('**/img/upload', route => {
    uploads++;
    const digit = uploads === 1 ? '1' : '3';
    route.fulfill({ contentType: 'application/json', body: JSON.stringify({
      upload_id: digit.repeat(32), upload_capability: (uploads === 1 ? '2' : '4').repeat(64),
      resto: '1000201', state: 'queued',
    }) });
  });
  let statusCalls = 0;
  await page.route('**/img/upload/status', route => {
    statusCalls++;
    return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...first, state: 'approved' }) });
  });
  await page.route('**/img/upload/cancel', route => {
    cancels++;
    if (denyCancel) route.fulfill({ status: 409, contentType: 'application/json', body: '{"error":"Upload is already in use."}' });
    else route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' });
  });
  await page.locator('.open-qr-link').click();
  const file = page.locator('#qrFile');
  await file.setInputFiles({ name: 'first.png', mimeType: 'image/png', buffer: Buffer.from([1]) });
  await expect(page.locator('#qrUploadStatus')).toContainText('queued');
  await expect.poll(() => statusCalls).toBe(1);
  await expect(page.locator('[name=upload_id]')).toHaveValue(first.upload_id, { timeout: 6_000 });
  await file.setInputFiles({ name: 'second.png', mimeType: 'image/png', buffer: Buffer.from([2]) });
  await expect(page.locator('#qrError')).toHaveText('Upload is already in use.');
  expect(uploads).toBe(1); expect(cancels).toBe(1);
  await expect(page.locator('[name=upload_id]')).toHaveValue(first.upload_id);
  denyCancel = false;
  await file.setInputFiles({ name: 'second.png', mimeType: 'image/png', buffer: Buffer.from([2]) });
  await expect.poll(() => uploads).toBe(2);
  expect(cancels).toBe(2);
});

test('inline drop accepts exactly one file, finite polling exposes Check status, and close cancels owned authority', async ({ page }) => {
  await page.clock.install();
  await page.goto('/img/thread/1000201');
  const receipt = { upload_id: '5'.repeat(32), upload_capability: '6'.repeat(64), resto: '1000201' };
  let status = 0, cancelled = 0;
  await page.route('**/img/upload', route => route.fulfill({
    contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'queued' }),
  }));
  await page.route('**/img/upload/status', route => {
    status++;
    route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'processing' }) });
  });
  await page.route('**/img/upload/cancel', route => {
    cancelled++;
    route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' });
  });
  await page.locator('.open-qr-link').click();
  const row = page.locator('.qr-file-row');
  await row.dispatchEvent('drop', { dataTransfer: await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([new Uint8Array([7])], 'dropped.png', { type: 'image/png' }));
    return transfer;
  }) });
  await expect(page.locator('#qrUploadStatus')).toContainText('queued');
  await page.clock.runFor(1_100); await expect.poll(() => status).toBe(1);
  await page.clock.runFor(2_100); await expect.poll(() => status).toBe(2);
  await page.clock.runFor(4_100); await expect.poll(() => status).toBe(3);
  expect(status).toBe(3);
  await expect(page.getByRole('button', { name: 'Check status', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Check status', exact: true }).click();
  await expect.poll(() => status).toBe(4);
  await expect(page.getByRole('button', { name: 'Check status', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await expect(page.locator('#quickReply')).toHaveCount(0);
  await expect.poll(() => cancelled).toBe(1);
});

test('inline Cancel aborts an in-flight upload before any receipt can become posting authority', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  let held, statusCalls = 0, cancelCalls = 0;
  await page.route('**/img/upload', route => { held = route; });
  await page.route('**/img/upload/status', route => { statusCalls++; return route.abort(); });
  await page.route('**/img/upload/cancel', route => { cancelCalls++; return route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' }); });
  await page.locator('.open-qr-link').click();
  await page.locator('#qrFile').setInputFiles({ name: 'held.png', mimeType: 'image/png', buffer: Buffer.from([9]) });
  await expect(page.locator('#qrUploadStatus')).toContainText('Uploading');
  await expect(page.getByRole('button', { name: 'Cancel file', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel file', exact: true }).click();
  await expect(page.locator('#qrUploadStatus')).toHaveText('');
  await expect(page.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
  expect(statusCalls).toBe(0); expect(cancelCalls).toBe(0);
  await held.fulfill({ contentType: 'application/json', body: JSON.stringify({
    upload_id: '7'.repeat(32), upload_capability: '8'.repeat(64), resto: '1000201', state: 'queued',
  }) }).catch(() => {});
  await page.waitForTimeout(50);
  await expect(page.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
});

test('approved inline authority survives a server posting error and is retired after an ambiguous posting failure', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  const receipt = { upload_id: '9'.repeat(32), upload_capability: 'a'.repeat(64), resto: '1000201' };
  await page.route('**/img/upload', route => route.fulfill({
    contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'queued' }),
  }));
  await page.route('**/img/upload/status', route => route.fulfill({
    contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'approved' }),
  }));
  let posts = 0;
  await page.route('**/img/imgboard.php', route => {
    posts++;
    if (posts === 1) return route.fulfill({ status: 422, contentType: 'application/json', body: '{"error":"Comment required."}' });
    return route.abort('failed');
  });
  await page.locator('.open-qr-link').click();
  const qr = page.locator('#quickReply');
  await qr.locator('#qrFile').setInputFiles({ name: 'approved.png', mimeType: 'image/png', buffer: Buffer.from([3]) });
  await expect(qr.locator('[name=upload_id]')).toHaveValue(receipt.upload_id, { timeout: 6_000 });
  await expect(qr.locator('#qr-pwd')).toHaveValue('');
  await qr.locator('input[type=submit]').click();
  await expect(qr.locator('#qrError')).toHaveText('Comment required.');
  await expect(qr.locator('[name=upload_id]')).toHaveValue(receipt.upload_id);
  await qr.locator('input[type=submit]').click();
  await expect(qr.locator('#qrError')).toContainText('Check the thread');
  await expect(qr.locator('[name=upload_id], [name=upload_capability]')).toHaveCount(0);
  await expect(qr.locator('[name=spoiler]')).toBeDisabled();
});

test('disable and persisted pagehide cancel owned inline authority and reject late status results', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  let sequence = 0, cancels = 0, heldStatus;
  await page.route('**/img/upload', route => {
    sequence++;
    const digit = sequence === 1 ? 'b' : 'd';
    return route.fulfill({ contentType: 'application/json', body: JSON.stringify({
      upload_id: digit.repeat(32), upload_capability: (sequence === 1 ? 'c' : 'e').repeat(64),
      resto: '1000201', state: 'queued',
    }) });
  });
  await page.route('**/img/upload/status', route => { heldStatus = route; });
  await page.route('**/img/upload/cancel', route => {
    cancels++; return route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' });
  });
  await page.locator('.open-qr-link').click();
  await page.locator('#qrFile').setInputFiles({ name: 'disable.png', mimeType: 'image/png', buffer: Buffer.from([1]) });
  await expect(page.locator('#qrUploadStatus')).toContainText('queued');
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect(page.locator('#quickReply')).toHaveCount(0);
  await expect.poll(() => cancels).toBe(1);
  await heldStatus?.fulfill({ contentType: 'application/json', body: JSON.stringify({
    upload_id: 'b'.repeat(32), upload_capability: 'c'.repeat(64), resto: '1000201', state: 'approved',
  }) }).catch(() => {});

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', '{}');
    dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await page.locator('.open-qr-link').click();
  await page.locator('#qrFile').setInputFiles({ name: 'cache.png', mimeType: 'image/png', buffer: Buffer.from([2]) });
  await expect(page.locator('#qrUploadStatus')).toContainText('queued');
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await expect(page.locator('#quickReply')).toHaveCount(0);
  await expect.poll(() => cancels).toBe(2);
});

test('closing an approved inline upload resets the reopened editor', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  const receipt = { upload_id: '1'.repeat(32), upload_capability: '2'.repeat(64), resto: '1000201' };
  await page.route('**/img/upload', route => route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'queued' }) }));
  await page.route('**/img/upload/status', route => route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'approved' }) }));
  await page.route('**/img/upload/cancel', route => route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' }));
  await page.locator('.open-qr-link').click();
  await page.locator('#qrFile').setInputFiles({ name: 'closed.png', mimeType: 'image/png', buffer: Buffer.from([1]) });
  await expect(page.locator('#quickReply [name=upload_id]')).toHaveValue(receipt.upload_id);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.locator('.open-qr-link').click();
  await expect(page.locator('#qrUploadStatus')).toHaveText('');
  await expect(page.locator('#quickReply [name=spoiler]')).toBeDisabled();
  await expect(page.locator('#quickReply [name=upload_id], #quickReply [name=upload_capability]')).toHaveCount(0);
});

test('posting freezes inline attachment replacement and cancellation', async ({ page }) => {
  await page.goto('/img/thread/1000201');
  const receipt = { upload_id: '3'.repeat(32), upload_capability: '4'.repeat(64), resto: '1000201' };
  let uploads = 0, cancels = 0, pending;
  await page.route('**/img/upload', route => { uploads++; return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'queued' }) }); });
  await page.route('**/img/upload/status', route => route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ...receipt, state: 'approved' }) }));
  await page.route('**/img/upload/cancel', route => { cancels++; return route.fulfill({ contentType: 'application/json', body: '{"cancelled":true}' }); });
  await page.route('**/img/imgboard.php', route => { pending = route; });
  await page.locator('.open-qr-link').click();
  await page.locator('#qrFile').setInputFiles({ name: 'posting.png', mimeType: 'image/png', buffer: Buffer.from([1]) });
  await expect(page.locator('#quickReply [name=upload_id]')).toHaveValue(receipt.upload_id);
  await expect(page.locator('#qr-pwd')).toHaveValue('');
  await page.locator('#quickReply input[type=submit]').click();
  await expect.poll(() => !!pending).toBe(true);
  await expect(page.getByRole('button', { name: 'Cancel file', exact: true })).toBeDisabled();
  await page.locator('.qr-file-row').dispatchEvent('drop', { dataTransfer: await page.evaluateHandle(() => {
    const transfer = new DataTransfer(); transfer.items.add(new File([new Uint8Array([2])], 'replacement.png', { type: 'image/png' })); return transfer;
  }) });
  await pending.fulfill({ contentType: 'application/json', body: '{"error":"Correct the post."}' });
  await expect(page.locator('#qrError')).toHaveText('Correct the post.');
  expect(uploads).toBe(1); expect(cancels).toBe(0);
  await expect(page.locator('#quickReply [name=upload_id]')).toHaveValue(receipt.upload_id);
});

test('an approved Quick Reply consumes both editors capability fields and reopening permits text only', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
  await page.goto('/demo/upload/fixture');
  const native = page.locator('form.postEditor').first();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  const qr = page.locator('#quickReply'); await expect(qr.locator('[name=upload_id]')).toHaveValue('1'.repeat(32));
  await expect(page.locator('#qr-pwd')).toHaveValue(''); await qr.locator('[name=spoiler]').check();
  await page.route('**/demo/imgboard.php', route => route.fulfill({ contentType: 'application/json', body: '{"tid":1000001,"pid":1000002}' }));
  await qr.locator('input[type=submit]').click();
  await expect(qr.locator('[name=upload_id], [name=upload_capability], [name=spoiler]')).toHaveCount(0);
  await expect(native.locator('[name=upload_id], [name=upload_capability], [name=spoiler]')).toHaveCount(0);
  await expect(native.locator('[name=com]')).toHaveAttribute('required', '');
  await expect(native.getByRole('button', { name: 'Post', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  await expect(qr).toBeVisible(); await expect(qr.locator('[name=upload_id], [name=upload_capability], [name=spoiler]')).toHaveCount(0);
  await expect(qr.locator('input[type=submit]')).toBeEnabled();
});

test('source length advice is debounced, typed, cancelable and does not hide server errors', async ({ page }) => {
  await page.goto('/demo/'); await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click();
  await expect(page.locator('#qrResto')).toHaveValue('1000001');
  await expect(page.locator('#quickReply')).toHaveAttribute('data-trackpos', 'QR-position');
  const time = new Date('2026-09-14T00:00:00Z'); await page.clock.install({ time }); await page.clock.pauseAt(time);
  const limit = Number(await page.locator('form.postEditor').first().getAttribute('data-comment-limit'));
  const value = '😀'.repeat(Math.floor(limit / 4) + 1), bytes = new TextEncoder().encode(value).length;
  const comment = page.locator('#qrCom'), error = page.locator('#qrError');
  await comment.fill(value); await comment.press('ArrowLeft');
  await page.clock.runFor(499); await expect(error).toBeHidden();
  await comment.press('ArrowRight'); await page.clock.runFor(499); await expect(error).toBeHidden();
  await page.clock.runFor(1); await expect(error).toHaveText(`Error: Comment too long (${bytes}/${limit}).`);
  await expect(error).toHaveAttribute('data-type', 'length'); await expect(page.locator('#quickReply input[type=submit]')).toBeEnabled();
  await comment.fill('Short'); await comment.press('ArrowLeft'); await page.clock.runFor(500); await expect(error).toBeHidden();
  await comment.evaluate((node, value) => { node.dispatchEvent(new Event('paste')); node.value = value; }, value);
  await page.clock.runFor(500); await expect(error).toHaveText(`Error: Comment too long (${bytes}/${limit}).`);
  await comment.evaluate(node => { node.dispatchEvent(new Event('cut')); node.value = 'Short'; });
  await page.clock.runFor(500); await expect(error).toBeHidden();
  await expect(page.locator('#qr-pwd')).toHaveValue('');
  await page.route('**/demo/imgboard.php', route => route.fulfill({ contentType: 'application/json', body: '{"error":"Server rule rejected this post"}' }));
  await page.locator('#quickReply input[type=submit]').click(); await expect(error).toHaveText('Server rule rejected this post');
  await comment.press('ArrowRight'); await page.clock.runFor(500); await expect(error).toHaveText('Server rule rejected this post');
  await comment.fill(value); await comment.press('ArrowLeft'); await comment.press('Escape');
  await page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first().click(); await page.clock.runFor(1000);
  await expect(page.locator('#qrError')).toBeHidden(); await expect(comment).toHaveValue('>>1000001\n');
});

test('source mobile reopening uses 25px while initial placement uses 28px and prefix quotes retain editing position', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 }); await page.goto('/demo/');
  // Keep the quote target below the overlay while exercising repeated real clicks.
  await page.locator('#mpostform a').click();
  const link = page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first(); await link.click();
  expect(await page.locator('#quickReply').evaluate(node => parseFloat(node.style.top) - scrollY)).toBe(28);
  await link.click(); expect(await page.locator('#quickReply').evaluate(node => parseFloat(node.style.top) - scrollY)).toBe(25);
  const comment = page.locator('#qrCom'); await comment.fill('Long line\n'.repeat(100));
  await comment.evaluate(node => { node.setSelectionRange(0, 0); node.scrollTop = 0; });
  await link.click();
  expect(await comment.evaluate(node => node.selectionStart)).toBe('>>1000001\n'.length);
  expect(await comment.evaluate(node => node.scrollTop < node.scrollHeight - node.clientHeight)).toBe(true);
  await comment.evaluate(node => node.setSelectionRange(node.value.length, node.value.length)); await link.click();
  expect(await comment.evaluate(node => node.selectionStart === node.value.length)).toBe(true);
  expect(await comment.evaluate(node => Math.abs(node.scrollTop + node.clientHeight - node.scrollHeight) <= 1)).toBe(true);
});

test('source Q is thread-only and quotes selection without inventing a post link', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true })));
  await page.goto('/demo/'); await page.getByRole('heading', { level: 1 }).click(); await page.keyboard.press('q');
  await expect(page.locator('#quickReply')).toHaveCount(0);
  await page.goto('/img/thread/1000201'); await page.getByRole('heading', { level: 1 }).click();
  const selected = await page.getByRole('heading', { level: 1 }).textContent();
  await page.getByRole('heading', { level: 1 }).evaluate(node => { const range = document.createRange(); range.selectNodeContents(node); getSelection().removeAllRanges(); getSelection().addRange(range); });
  await page.keyboard.press('q'); await expect(page.locator('#qrCom')).toHaveValue(`>${selected}\n`);
  await expect(page.locator('#qrResto')).toHaveValue('1000201');
});

test('Ctrl-click quotes without linking even with optional keyboard shortcuts disabled', async ({ page, context }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: false })));
  await page.goto('/demo/'); const link = page.locator(':is(.postInfo, .postInfoM) > .postNum > a[title="Reply to this post"]:visible').first();
  await link.click({ modifiers: ['Control'] }); await expect(page.locator('#qrCom')).toHaveValue('');
  expect(context.pages()).toHaveLength(1);
  await page.locator('#qrCom').fill('Existing draft');
  const selected = await page.getByRole('heading', { level: 1 }).textContent();
  await page.getByRole('heading', { level: 1 }).evaluate(node => { const range = document.createRange(); range.selectNodeContents(node); getSelection().removeAllRanges(); getSelection().addRange(range); });
  // Dispatch the source click with an existing DOM selection, without a
  // preceding synthetic mousedown that would collapse it in this harness.
  const selection = await page.locator('#qrCom').evaluate(node => ({ start: node.selectionStart, end: node.selectionEnd }));
  const expected = 'Existing draft'.slice(0, selection.start) + `>${selected}\n` + 'Existing draft'.slice(selection.end);
  await link.dispatchEvent('click', { ctrlKey: true, button: 0 });
  await expect(page.locator('#qrCom')).toHaveValue(expected);
  await page.locator('#qrCom').selectText(); await page.locator('#qrCom').press('Control+s');
  await expect(page.locator('#qrCom')).toHaveValue(`[spoiler]${expected}[/spoiler]`);
  await page.locator('#qrCom').press('Escape'); await expect(page.locator('#quickReply')).toHaveCount(0);
  expect(context.pages()).toHaveLength(1);
});
