import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} Quick Reply preserves quote editing and fits desktop/mobile`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/demo/');
      const link = page.locator('.postInfo > .postNum').first(); const id = (await link.textContent()).slice(3);
      await link.click();
      const dialog = page.locator('#quickReply'); await expect(dialog).toBeVisible();
      await expect(page.locator('#qrCom')).toHaveValue(`>>${id}\n`);
      await expect(page.locator('#qrCom')).toBeFocused();
      await page.locator('#qrCom').fill('Selected fold'); await page.locator('#qrCom').selectText();
      await page.locator('#qrCom').press('Control+s');
      await expect(page.locator('#qrCom')).toHaveValue('[spoiler]Selected fold[/spoiler]');
      await page.locator('#qr-pwd').fill('owned-synthetic-password');
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
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  const header = page.locator('#qrHeader'), before = await header.boundingBox();
  await page.mouse.move(before.x + 30, before.y + 8); await page.mouse.down();
  await page.mouse.move(60, 65); await page.mouse.up();
  const after = await page.locator('#quickReply').boundingBox();
  expect(after.x).toBeGreaterThanOrEqual(0); expect(after.y).toBeGreaterThanOrEqual(0);
  expect(after.x).toBeLessThan(before.x);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  const restored = await page.locator('#quickReply').boundingBox(); expect(restored.x).toBe(after.x); expect(restored.y).toBe(after.y);
});

test('Quick Reply renders errors as text, aborts without retry and ignores late completions after close', async ({ page }) => {
  await page.goto('/demo/'); await page.locator('.postInfo > .postNum').first().click();
  await page.locator('#qrCom').fill('Owned draft'); await page.locator('#qr-pwd').fill('owned-password');
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
  await page.locator('.postInfo > .postNum').first().click();
  await expect(page.locator('#qrCom')).toHaveValue('>>1000001\n');
  await expect(submit).toHaveValue('Post'); await expect(page.locator('#com')).toHaveValue('');
});

test('Quick Reply guards closed threads and keeps the source spoiler caret behavior', async ({ page }) => {
  await page.goto('/demo/'); await page.locator('.postInfo > .postNum').first().click();
  const comment = page.locator('#qrCom');
  await comment.fill(''); await comment.press('Control+s');
  expect(await comment.evaluate(node => node.selectionStart)).toBe(9);
  await comment.fill('suffix'); await comment.evaluate(node => node.setSelectionRange(0, 0)); await comment.press('Control+s');
  await expect(comment).toHaveValue('[spoiler][/spoiler]suffix'); expect(await comment.evaluate(node => node.selectionStart)).toBe(19);
  await page.evaluate(() => { document.querySelector('.thread').dataset.closed = 'true'; document.dispatchEvent(new Event('boardThreadStateChanged')); });
  await expect(page.locator('#quickReply input[type=submit]')).toBeDisabled(); await expect(page.locator('#qrError')).toHaveText('This thread is closed.');
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await page.locator('.postInfo > .postNum').first().click(); await expect(page.locator('#quickReply')).toHaveCount(0);
});

test('an approved Quick Reply consumes both editors capability fields and reopening permits text only', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
  await page.goto('/demo/upload/fixture');
  const native = page.locator('form.postEditor').first();
  await page.getByRole('link', { name: 'Post a Reply', exact: true }).click();
  const qr = page.locator('#quickReply'); await expect(qr.locator('[name=upload_id]')).toHaveValue('1'.repeat(32));
  await page.locator('#qr-pwd').fill('owned-password'); await qr.locator('[name=spoiler]').check();
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
  await page.goto('/demo/'); await page.locator('.postInfo > .postNum').first().click();
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
  await page.locator('#qr-pwd').fill('owned-password');
  await page.route('**/demo/imgboard.php', route => route.fulfill({ contentType: 'application/json', body: '{"error":"Server rule rejected this post"}' }));
  await page.locator('#quickReply input[type=submit]').click(); await expect(error).toHaveText('Server rule rejected this post');
  await comment.press('ArrowRight'); await page.clock.runFor(500); await expect(error).toHaveText('Server rule rejected this post');
  await comment.fill(value); await comment.press('ArrowLeft'); await comment.press('Escape');
  await page.locator('.postInfo > .postNum').first().click(); await page.clock.runFor(1000);
  await expect(page.locator('#qrError')).toBeHidden(); await expect(comment).toHaveValue('>>1000001\n');
});

test('source mobile reopening uses 25px while initial placement uses 28px and prefix quotes retain editing position', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 }); await page.goto('/demo/');
  const link = page.locator('.postInfo > .postNum').first(); await link.click();
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
