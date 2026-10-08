import { withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { ownedDeletionMarker, deletionFixture } from './helpers/deletion-fixture.js';
import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { installPostingResponseObserver } from './helpers/posting-response.js';

const origin = 'http://127.0.0.1:3000';

test('desktop and mobile posting retain robot errors and Quick Reply drafts without creating rejected posts', async ({ browser, request }) => {
  const marker = ownedDeletionMarker();
  const password = `owned-${randomUUID()}`;
  const created = await withPostingHistory(() => request.post('/r9k/post', {
    headers: { Origin: origin }, maxRedirects: 0, form: { sub: marker, com: marker, password },
  }));
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
        await expect(page.locator('#postPassword')).toHaveValue('');
        await withPostingHistory(() => page.locator('form.postEditor button[type="submit"]').click());
        await expect(page.getByText('Non-ASCII text is not allowed.', { exact: true })).toBeVisible();
        await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, persistentQR: true, threadWatcher: false })));
        await page.goto(`${origin}/r9k/thread/${id}`);
        await page.locator(`#${mobile ? 'pim' : 'pi'}${id} > .postNum > a[title="Reply to this post"]`).click();
        await expect(page.locator('#qr-pwd')).toHaveValue('');
        await page.locator('#qrCom').fill('café');
        await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click());
        await expect(page.locator('#qrError')).toHaveText('Non-ASCII text is not allowed.');
        await expect(page.locator('#qrCom')).toHaveValue('café');
        const original = `${marker} unique reply at width ${width}`;
        const qrUrl = page.url();
        const navigations = [];
        page.on('framenavigated', frame => { if (frame === page.mainFrame()) navigations.push(frame.url()); });
        await page.locator('#qrCom').fill(original);
        await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click());
        await expect(page.locator('.postMessage').filter({ hasText: original })).toHaveCount(1);
        await expect(page.locator('#qrCom')).toHaveValue('');
        await expect(page.locator('#quickReply input[type=submit]')).not.toHaveValue('Sending');
        expect(page.url()).toBe(qrUrl);
        expect(navigations).toEqual([]);
        if (mobile) {
          await page.locator('#qrCom').fill(marker);
          // Capture the real body before the transport aborts its completed fetch.
          // CDP may discard it by the time a later Response.json() asks for it.
          await page.evaluate(installPostingResponseObserver, { url: `${origin}/r9k/imgboard.php`, thread: id, comment: marker });
          const response = page.evaluate(() => window.ownedPostingResponse);
          // Bypass the client advisory to exercise the server's duplicate-post rejection.
          const [, rejected] = await Promise.all([
            withPostingHistory(() => page.locator('#quickReply input[type=submit]').click({ modifiers: ['Shift'] })), response,
          ]);
          expect(rejected.status).toBe(200);
          expect(rejected.type).toBe('application/json');
          expect(JSON.parse(rejected.text)).toEqual({ error: 'You have been muted for 2 seconds, because your comment was not original.' });
          await expect(page.locator('#qrError')).toHaveText('You have been muted for 2 seconds, because your comment was not original.');
          await expect(page.locator('#qrCom')).toHaveValue(marker);
          expect(page.url()).toBe(qrUrl);
          expect(navigations).toEqual([]);
        }
      } finally { await context.close(); }
    }
    const saved = await (await request.get(`/r9k/thread/${id}.json`)).json();
    expect(saved.posts).toHaveLength(3);
    expect(errors).toEqual([]);
  } finally {
    deletionFixture('cleanup', 'r9k', id, marker);
  }
});
