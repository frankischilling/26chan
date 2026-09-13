import { test, expect } from '@playwright/test';
import { saveWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-post-tracking-password';
const owned = [];
test.afterEach(async ({ request }) => {
  for (const id of owned.splice(0)) {
    const deleted = await request.post('/test/delete', { headers: { Origin: origin }, form: { no: id, password }, maxRedirects: 0 });
    expect(deleted.status()).toBe(303);
    expect((await request.get(`/test/thread/${id}.json`)).status()).toBe(404);
  }
});
async function post(page, comment, { subject = '', option = '' } = {}) {
  await page.locator('#com').fill(comment);
  await page.locator('#password').fill(password);
  if (subject) await page.locator('#sub').fill(subject);
  await page.locator('#email').selectOption(option);
  const response = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/test/post'));
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  expect((await response).status()).toBe(303);
}
async function autoWatch(page) {
  await page.goto('/test/');
  await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true });
  await expect(page.locator('input[name=awt]')).toHaveValue('1');
}
test('successful ordinary and board-return posts auto-watch, track own replies and clear receipts', async ({ page, context, request }) => {
  await page.clock.install();
  await autoWatch(page);
  await post(page, 'Owned new thread', { subject: 'Automatically watched paper model' });
  await expect(page).toHaveURL(/\/test\/thread\/(\d+)#p\d+$/);
  const thread = page.url().match(/thread\/(\d+)/)[1];
  owned.push(thread);
  await expect(page.locator(`#watch-${thread}-test`)).toContainText('Automatically watched paper model');
  await expect.poll(() => page.evaluate(thread => JSON.parse(localStorage.getItem(`4chan-track-test-${thread}`))?.[`>>${thread}`], thread)).toBe(1);
  await post(page, 'My reply', { option: 'nonoko' });
  await expect(page).toHaveURL(origin + '/test/');
  const json = await (await request.get(`/test/thread/${thread}.json`)).json();
  const ownReply = String(json.posts.at(-1).no);
  await expect.poll(() => page.evaluate(({ thread, ownReply }) => JSON.parse(localStorage.getItem(`4chan-track-test-${thread}`))?.[`>>${ownReply}`], { thread, ownReply })).toBe(1);
  expect((await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-') || cookie.name === '4chan_awt')).toEqual([]);
  await expect(page.locator('.watcherNotice')).toContainText('Refresh complete.');
  const other = await request.post('/test/post', { headers: { Origin: origin },
    form: { resto: thread, com: `>>${ownReply}\nA reply to your fold`, password }, maxRedirects: 0 });
  expect(other.status()).toBe(303);
  await page.clock.fastForward(60001);
  const refreshed = page.waitForResponse(response => response.url().endsWith(`/_watch/test/thread/${thread}.json`));
  await page.goto('/test/catalog?q=');
  expect((await refreshed).status()).toBe(200);
  const watched = page.locator(`#watch-${thread}-test a`);
  await expect(watched).toHaveClass(/hasYouReplies/);
  await expect(watched).toHaveAttribute('title', 'This thread has replies to your posts');
  await expect(watched).toHaveText('(1) /test/ - Automatically watched paper model');
});
test('concurrent successful posts use distinct receipts and failed posts create none', async ({ page, context }) => {
  await autoWatch(page);
  const results = await Promise.all(['First concurrent thread', 'Second concurrent thread'].map(sub => context.request.post('/test/post', {
    headers: { Origin: origin }, form: { sub, com: 'Owned concurrent fixture', password, track: '1', awt: '1' }, maxRedirects: 0,
  })));
  const ids = results.map(response => { expect(response.status()).toBe(303); return response.headers().location.match(/thread\/(\d+)/)[1]; });
  owned.push(...ids);
  const cookies = (await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-'));
  expect(cookies.map(cookie => cookie.name).sort()).toEqual(ids.map(id => `board-posted-${id}`).sort());
  for (const cookie of cookies) { expect(cookie.path).toBe('/test/'); expect(cookie.sameSite).toBe('Strict'); expect(cookie.httpOnly).toBe(false); }
  await page.goto('/test/');
  for (const id of ids) {
    await expect(page.locator(`#watch-${id}-test`)).toBeVisible();
    await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem(`4chan-track-test-${id}`))?.[`>>${id}`], id)).toBe(1);
  }
  const before = await page.evaluate(() => localStorage.getItem('4chan-watch'));
  const failed = await context.request.post('/test/post', { headers: { Origin: origin }, form: { com: '', password, track: '1', awt: '1' }, maxRedirects: 0 });
  expect(failed.status()).toBe(422);
  expect(failed.headers()['set-cookie']).toBeUndefined();
  await page.goto('/test/catalog?q=');
  expect(await page.evaluate(() => localStorage.getItem('4chan-watch'))).toBe(before);
  expect((await context.cookies()).some(cookie => cookie.name.startsWith('board-posted-'))).toBe(false);
});
test('disabled extension and no-JavaScript posting keep ordinary forms and redirects', async ({ page, browser }) => {
  await page.goto('/test/');
  await page.evaluate(() => localStorage.setItem('4chan-settings', '{"disableAll":true,"threadWatcher":true,"threadAutoWatcher":true}'));
  await page.reload();
  await expect(page.locator('input[name=track], input[name=awt]')).toHaveCount(0);
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const plain = await context.newPage();
    await plain.goto(origin + '/test/');
    await expect(plain.locator('input[name=track], input[name=awt]')).toHaveCount(0);
    await post(plain, 'Owned no-JavaScript tracking control');
    await expect(plain).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
    owned.push(plain.url().match(/thread\/(\d+)/)[1]);
    expect((await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-') || cookie.name === '4chan_awt')).toEqual([]);
  } finally { await context.close(); }
});
