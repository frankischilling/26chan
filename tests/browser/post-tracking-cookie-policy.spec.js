import { test, expect, chromium } from '@playwright/test';
import { mkdtemp, mkdir, writeFile, readFile, realpath, rm } from 'node:fs/promises';
import path from 'node:path';
import { saveWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-cookie-policy-password';
const receiptName = name => name === '4chan_awt' || name.startsWith('board-posted-');

test('real browser cookie rejection preserves posts and cannot defer rejected tracking receipts', async ({ request }) => {
  test.setTimeout(60000);
  const base = await realpath('.local');
  const owned = new Set();
  try {
    // Full Chromium is required: its Chrome profile honors this content setting.
    // The default headless-shell executable does not apply these preferences.
    for (const cookies of [1, 2]) {
      const profile = await mkdtemp(path.join(base, 'posting-cookie-policy-'));
      let context;
      try {
        await mkdir(path.join(profile, 'Default'));
        const preferences = path.join(profile, 'Default', 'Preferences');
        await writeFile(preferences, JSON.stringify({ profile: { default_content_setting_values: { cookies } } }));
        const launch = () => chromium.launchPersistentContext(profile, {
          channel: 'chromium', headless: true, baseURL: origin, viewport: { width: 1280, height: 900 },
          locale: 'en-US', timezoneId: 'America/New_York', timeout: 15000,
        });
        context = await launch();
        const page = context.pages()[0]; page.setDefaultTimeout(10000);
        const errors = []; page.on('pageerror', error => errors.push(error.message));
        const cdp = await context.newCDPSession(page);
        const blocked = [];
        cdp.on('Network.responseReceivedExtraInfo', event => {
          for (const cookie of event.blockedCookies) {
            if (/^(board-posted-[0-9]+|4chan_awt)=/.test(cookie.cookieLine)) blocked.push(cookie);
          }
        });
        await cdp.send('Network.enable');
        await page.goto('/test/');
        await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true }, { reload: false });
        await expect(page.locator('form.postEditor input[name=track]')).toHaveValue('1');
        await expect(page.locator('form.postEditor input[name=awt]')).toHaveValue('1');
        const post = async (parent, option = '') => {
          await page.locator('#togglePostFormLink a').click();
          await page.locator('#com').fill(`Owned cookie policy ${cookies}, parent ${parent}`);
          await page.locator('#password').fill(password);
          await page.locator('#email').fill(option);
          if (parent === '0') await page.locator('#sub').fill('Owned network cookie policy');
          const pending = page.waitForResponse(response => response.request().method() === 'POST'
            && response.url() === `${origin}/test/imgboard.php`);
          await page.getByRole('button', { name: 'Post', exact: true }).click();
          const response = await pending;
          expect(response.status()).toBe(303);
          if (parent === '0') {
            const location = await response.headerValue('location');
            const thread = location.match(/\/thread\/(\d+)/)?.[1];
            expect(thread).toBeTruthy(); owned.add(thread);
          }
          return (await response.headersArray()).filter(header => header.name.toLowerCase() === 'set-cookie').map(header => header.value);
        };
        const opHeaders = await post('0');
        await expect(page).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
        const thread = page.url().match(/thread\/(\d+)/)[1];
        expect(opHeaders).toHaveLength(2);
        expect(opHeaders.some(line => line.startsWith(`board-posted-${thread}=${thread}.1;`))).toBe(true);
        expect(opHeaders.some(line => line.startsWith(`4chan_awt=${thread};`))).toBe(true);
        const assertReceipts = async (headers, postId) => {
          if (cookies === 1) {
            await expect.poll(() => page.evaluate(({ thread, postId }) =>
              JSON.parse(localStorage.getItem(`4chan-track-test-${thread}`))?.[`>>${postId}`], { thread, postId })).toBe(1);
            await expect(page.locator(`#watch-${thread}-test`)).toBeVisible();
            expect(blocked).toEqual([]);
          } else {
            for (const line of headers) {
              await expect.poll(() => blocked.some(cookie => cookie.cookieLine === line
                && cookie.blockedReasons.includes('UserPreferences'))).toBe(true);
            }
            await expect(page.locator(`#watch-${thread}-test`)).toHaveCount(0);
            expect(await page.evaluate(() => document.cookie)).toBe('');
          }
          expect((await context.cookies()).filter(cookie => receiptName(cookie.name))).toEqual([]);
        };
        await assertReceipts(opHeaders, thread);
        // Blocking cookies may also block persistent web storage. Re-enable the
        // real controls in the current tab before testing the next submission.
        if (cookies === 2) await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true }, { reload: false });
        await expect(page.locator('form.postEditor input[name=track]')).toHaveValue('1');
        const replyHeaders = await post(thread, 'nonoko');
        await expect(page).toHaveURL(`${origin}/test/`);
        expect(replyHeaders).toHaveLength(1);
        const reply = replyHeaders[0].match(/^board-posted-(\d+)=/)?.[1];
        expect(reply).toBeTruthy();
        expect(replyHeaders[0].startsWith(`board-posted-${reply}=${thread}.0;`)).toBe(true);
        await assertReceipts(replyHeaders, reply);
        const jsonResponse = await request.get(`/test/thread/${thread}.json`);
        expect(jsonResponse.status()).toBe(200);
        const posts = (await jsonResponse.json()).posts;
        expect(posts.map(post => String(post.no))).toEqual([thread, reply]);
        expect(posts[1].com).toBe(`Owned cookie policy ${cookies}, parent ${thread}`);
        expect(errors).toEqual([]);
        if (cookies === 2) {
          await context.close(); context = null;
          // Change only the owned profile while its browser is stopped.
          const restored = JSON.parse(await readFile(preferences, 'utf8'));
          restored.profile.default_content_setting_values.cookies = 1;
          await writeFile(preferences, JSON.stringify(restored));
          context = await launch();
          const restoredPage = context.pages()[0]; restoredPage.setDefaultTimeout(10000);
          await restoredPage.goto(`/test/thread/${thread}`);
          await saveWatcherSettings(restoredPage, { threadWatcher: true, threadAutoWatcher: true });
          expect(await restoredPage.evaluate(thread => localStorage.getItem(`4chan-track-test-${thread}`), thread)).toBe(null);
          await expect(restoredPage.locator(`#watch-${thread}-test`)).toHaveCount(0);
          expect((await context.cookies()).filter(cookie => receiptName(cookie.name))).toEqual([]);
          await expect(restoredPage.locator(`#m${reply}`)).toHaveText(`Owned cookie policy 2, parent ${thread}`);
        }
      } finally {
        if (context) await context.close();
        expect(path.dirname(await realpath(profile))).toBe(base);
        expect(path.basename(profile).startsWith('posting-cookie-policy-')).toBe(true);
        await rm(profile, { recursive: true, maxRetries: 5, retryDelay: 100 });
      }
    }
  } finally {
    for (const id of owned) {
      const deleted = await request.post('/test/delete', { headers: { Origin: origin },
        form: { no: id, password }, maxRedirects: 0 });
      expect(deleted.status()).toBe(303);
      expect((await request.get(`/test/thread/${id}.json`)).status()).toBe(404);
    }
  }
});
