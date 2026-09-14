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
