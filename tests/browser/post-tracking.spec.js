import { withDeletionQuota } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { saveWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-post-tracking-password';
const owned = [];
test.afterEach(async ({ request }) => {
  for (const { id, cookie } of owned.splice(0)) {
    await withDeletionQuota(async () => {
      const deleted = await request.post('/fixture/delete', { headers: { Origin: origin, Cookie: cookie }, form: { no: id }, maxRedirects: 0 });
      expect(deleted.status()).toBe(303);
      expect((await request.get(`/fixture/thread/${id}.json`)).status()).toBe(404);
    });
  }
});
async function ownerCookie(response) {
  const cookies = (await response.headersArray()).filter(header => header.name.toLowerCase() === 'set-cookie' && header.value.startsWith('board-anon='));
  expect(cookies.length).toBe(1);
  return cookies[0].value.split(';', 1)[0];
}
async function post(page, comment, { subject = '', option = '', nativeControls = true } = {}) {
  if (nativeControls) await page.locator('#togglePostFormLink a').click();
  await page.locator('#com').fill(comment);
  await expect(page.locator('#postPassword')).toHaveValue('');
  if (subject) await page.locator('#sub').fill(subject);
  await page.locator('#email').fill(option);
  const response = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/fixture/imgboard.php'));
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  const posted = await response;
  expect(posted.status()).toBe(303);
  return ownerCookie(posted);
}
async function autoWatch(page) {
  await page.goto('/fixture/');
  await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true });
  await expect(page.locator('input[name=awt]')).toHaveValue('1');
}
test('successful ordinary and board-return posts auto-watch, track own replies and clear receipts', async ({ page, context, request }) => {
  await page.clock.install();
  await autoWatch(page);
  const cookie = await post(page, 'Owned new thread', { subject: 'Automatically watched paper model' });
  await expect(page).toHaveURL(/\/fixture\/thread\/(\d+)#p\d+$/);
  const thread = page.url().match(/thread\/(\d+)/)[1];
  owned.push({ id: thread, cookie });
  await expect(page.locator(`#watch-${thread}-fixture`)).toContainText('Automatically watched paper model');
  await expect.poll(() => page.evaluate(thread => JSON.parse(localStorage.getItem(`4chan-track-fixture-${thread}`))?.[`>>${thread}`], thread)).toBe(1);
  await post(page, 'My reply', { option: 'nonoko' });
  await expect(page).toHaveURL(origin + '/fixture/');
  const json = await (await request.get(`/fixture/thread/${thread}.json`)).json();
  const ownReply = String(json.posts.at(-1).no);
  await expect.poll(() => page.evaluate(({ thread, ownReply }) => JSON.parse(localStorage.getItem(`4chan-track-fixture-${thread}`))?.[`>>${ownReply}`], { thread, ownReply })).toBe(1);
  expect((await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-') || cookie.name === '4chan_awt')).toEqual([]);
  await expect(page.locator('.watcherNotice')).toContainText('Refresh complete.');
  const other = await request.post('/fixture/post', { headers: { Origin: origin },
    form: { resto: thread, com: `>>${ownReply}\nA reply to your fold`, password }, maxRedirects: 0 });
  expect(other.status()).toBe(303);
  await page.clock.fastForward(60001);
  const refreshed = page.waitForResponse(response => response.url().endsWith(`/_watch/fixture/thread/${thread}.json`));
  await page.goto('/fixture/catalog?q=');
  expect((await refreshed).status()).toBe(200);
  const watched = page.locator(`#watch-${thread}-fixture a`);
  await expect(watched).toHaveClass(/hasYouReplies/);
  await expect(watched).toHaveAttribute('title', 'This thread has replies to your posts');
  await expect(watched).toHaveText('(1) /fixture/ - Automatically watched paper model');
});
test('concurrent successful posts use distinct receipts and failed posts create none', async ({ page, context }) => {
  await autoWatch(page);
  const results = await Promise.all(['First concurrent thread', 'Second concurrent thread'].map(sub => context.request.post('/fixture/post', {
    headers: { Origin: origin }, form: { sub, com: 'Owned concurrent fixture', password, track: '1', awt: '1' }, maxRedirects: 0,
  })));
  const ids = results.map(response => { expect(response.status()).toBe(303); return response.headers().location.match(/thread\/(\d+)/)[1]; });
  for (const [index, id] of ids.entries()) owned.push({ id, cookie: await ownerCookie(results[index]) });
  const cookies = (await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-'));
  expect(cookies.map(cookie => cookie.name).sort()).toEqual(ids.map(id => `board-posted-${id}`).sort());
  for (const cookie of cookies) { expect(cookie.path).toBe('/fixture/'); expect(cookie.sameSite).toBe('Strict'); expect(cookie.httpOnly).toBe(false); }
  await page.goto('/fixture/');
  for (const id of ids) {
    await expect(page.locator(`#watch-${id}-fixture`)).toBeVisible();
    await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem(`4chan-track-fixture-${id}`))?.[`>>${id}`], id)).toBe(1);
  }
  const before = await page.evaluate(() => localStorage.getItem('4chan-watch'));
  const failed = await context.request.post('/fixture/post', { headers: { Origin: origin }, form: { com: '', password, track: '1', awt: '1' }, maxRedirects: 0 });
  expect(failed.status()).toBe(422);
  expect(failed.headers()['set-cookie']).toBeUndefined();
  await page.goto('/fixture/catalog?q=');
  expect(await page.evaluate(() => localStorage.getItem('4chan-watch'))).toBe(before);
  expect((await context.cookies()).some(cookie => cookie.name.startsWith('board-posted-'))).toBe(false);
});
test('disabled document cookie access preserves posting and defers receipt consumption until access returns', async ({ page, context, request }) => {
  await autoWatch(page);
  await page.evaluate(() => { document.cookie = 'owned-watch-cookie-control=1; Path=/fixture/; SameSite=Strict'; });
  expect(await page.evaluate(() => document.cookie)).toContain('owned-watch-cookie-control=1');
  const session = await context.newCDPSession(page);
  await session.send('Emulation.setDocumentCookieDisabled', { disabled: true });
  expect(await page.evaluate(() => {
    try { return document.cookie.includes('owned-watch-cookie-control=1'); }
    catch { return false; }
  })).toBe(false);
  const cookie = await post(page, 'Owned post while cookie access is disabled', { subject: 'Deferred receipt fixture' });
  await expect(page).toHaveURL(/\/fixture\/thread\/(\d+)#p\d+$/);
  const thread = page.url().match(/thread\/(\d+)/)[1];
  owned.push({ id: thread, cookie });
  await expect(page.locator(`#m${thread}`)).toHaveText('Owned post while cookie access is disabled');
  await expect(page.locator('input[name=track]')).toHaveValue('1');
  expect((await request.get(`/fixture/thread/${thread}.json`)).status()).toBe(200);
  // Network cookie storage still works. Only the actual document API is disabled.
  expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${thread}`)).toBe(true);
  await expect(page.locator(`#watch-${thread}-fixture`)).toHaveCount(0);
  expect(await page.evaluate(thread => localStorage.getItem(`4chan-track-fixture-${thread}`), thread)).toBeNull();
  await session.send('Emulation.setDocumentCookieDisabled', { disabled: false });
  await page.reload();
  await expect(page.locator(`#watch-${thread}-fixture`)).toBeVisible();
  await expect.poll(() => page.evaluate(thread => JSON.parse(localStorage.getItem(`4chan-track-fixture-${thread}`))?.[`>>${thread}`], thread)).toBe(1);
  expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${thread}`)).toBe(false);
});

test('disabled extension and no-JavaScript posting keep ordinary forms and redirects', async ({ page, browser }) => {
  await page.goto('/fixture/');
  await page.evaluate(() => localStorage.setItem('4chan-settings', '{"disableAll":true,"threadWatcher":true,"threadAutoWatcher":true}'));
  await page.reload();
  await expect(page.locator('input[name=track], input[name=awt]')).toHaveCount(0);
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const plain = await context.newPage();
    await plain.goto(origin + '/fixture/');
    await expect(plain.locator('input[name=track], input[name=awt]')).toHaveCount(0);
    const cookie = await post(plain, 'Owned no-JavaScript tracking control', { nativeControls: false });
    await expect(plain).toHaveURL(/\/fixture\/thread\/\d+#p\d+$/);
    owned.push({ id: plain.url().match(/thread\/(\d+)/)[1], cookie });
    expect((await context.cookies()).filter(cookie => cookie.name.startsWith('board-posted-') || cookie.name === '4chan_awt')).toEqual([]);
  } finally { await context.close(); }
});
