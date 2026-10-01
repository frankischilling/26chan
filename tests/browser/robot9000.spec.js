import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';

const origin = 'http://127.0.0.1:3000';

test('desktop and mobile posting retain robot errors and Quick Reply drafts without creating rejected posts', async ({ browser, request }) => {
  const marker = `OwnedRobot${randomUUID().replaceAll('-', '')}`;
  const password = `owned-${randomUUID()}`;
  const created = await request.post('/r9k/post', {
    headers: { Origin: origin }, maxRedirects: 0, form: { sub: marker, com: marker, password },
  });
  expect(created.status(), await created.text()).toBe(303);
  const id = created.headers().location.match(/thread\/(\d+)/)[1];
  const errors = [];
  try {
    for (const width of [1280, 390]) {
      const mobile = width === 390;
      const context = await browser.newContext({ viewport: { width, height: 900 }, ...(mobile ? {
        isMobile: true, hasTouch: true,
        userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36',
      } : {}) });
      try {
        await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false, persistentQR: true, threadWatcher: false })));
        const page = await context.newPage();
        page.on('pageerror', error => errors.push(error.message));
        await page.goto(`${origin}/r9k/thread/${id}`);
        // The ordinary form uses the real HTML response.
        await page.locator(`#${mobile ? 'pim' : 'pi'}${id} > .postNum > a[title="Reply to this post"]`).click();
        await page.locator('#com').fill('café');
        await page.locator('#password').fill(password);
        await page.locator('form.postEditor button[type="submit"]').click();
        await expect(page.getByText('Non-ASCII text is not allowed.', { exact: true })).toBeVisible();
        await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, persistentQR: true, threadWatcher: false })));
        await page.goto(`${origin}/r9k/thread/${id}`);
        await page.locator(`#${mobile ? 'pim' : 'pi'}${id} > .postNum > a[title="Reply to this post"]`).click();
        await page.locator('#qr-pwd').fill(password);
        await page.locator('#qrCom').fill('café');
        await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('#qrError')).toHaveText('Non-ASCII text is not allowed.');
        await expect(page.locator('#qrCom')).toHaveValue('café');
        const original = `${marker} unique reply at width ${width}`;
        await page.locator('#qrCom').fill(original);
        await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('.postMessage').filter({ hasText: original })).toHaveCount(1);
        await expect(page.locator('#qrCom')).toHaveValue('');
        if (mobile) {
          await page.locator('#qrCom').fill(marker);
          const response = page.waitForResponse(response => response.url() === `${origin}/r9k/imgboard.php` && response.request().method() === 'POST');
          await page.locator('#quickReply input[type=submit]').click();
          const rejected = await response;
          expect(rejected.status()).toBe(200);
          expect(await rejected.json()).toEqual({ error: 'You have been muted for 2 seconds, because your comment was not original.' });
          await expect(page.locator('#qrError')).toHaveText('You have been muted for 2 seconds, because your comment was not original.');
          await expect(page.locator('#qrCom')).toHaveValue(marker);
        }
      } finally { await context.close(); }
    }
    const saved = await (await request.get(`/r9k/thread/${id}.json`)).json();
    expect(saved.posts).toHaveLength(3);
    expect(errors).toEqual([]);
  } finally {
    expect((await request.post('/r9k/delete', { headers: { Origin: origin }, maxRedirects: 0,
      form: { no: id, password } })).status()).toBe(303);
  }
});
