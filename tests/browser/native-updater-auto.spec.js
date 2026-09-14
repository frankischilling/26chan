import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-auto-updater-password';
    const write = form => request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } });
    const response = await write({ resto: '0', sub: 'Owned auto updater', com: 'Original post' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    try { await use({ id, url: `/demo/thread/${id}`, path: `/_watch/demo/thread/${id}/posts`,
      reply: async com => { const response = await write({ resto: id, com }); expect(response.status()).toBe(303); return response.headers().location.match(/#p(\d+)/)[1]; } }); }
    finally { await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } }); }
  },
});
const auto = page => page.locator('.threadNav.desktop input[data-cmd="auto"]').first();
const status = page => page.locator('.threadNav.desktop .nativeUpdaterStatus').first();
const manual = page => page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
async function initialize(page, owned, settings = {}) {
  await page.addInitScript(settings => localStorage.setItem('4chan-settings', JSON.stringify(settings)), settings);
  await page.goto(owned.url);
  await expect(auto(page)).toBeVisible();
}
async function freeze(page) {
  const time = new Date('2026-09-13T18:00:00Z');
  await page.clock.install({ time }); await page.clock.pauseAt(time);
}
async function advance(page, seconds) { await page.clock.runFor(seconds * 1000); }

test('A and mirrored controls append persisted replies on schedule, preserve drafts and track unread posts', async ({ page, owned }) => {
  const previous = await owned.reply('Existing reply');
  await page.setViewportSize({ width: 1280, height: 400 });
  await initialize(page, owned, { keyBinds: true }); await freeze(page);
  const title = await page.title();
  await page.locator('#com').fill('Retained draft'); await page.keyboard.press('a');
  await expect(auto(page)).not.toBeChecked();
  await page.locator('h1').click(); await page.keyboard.press('a');
  for (const input of await page.locator('input[data-cmd="auto"]').all()) await expect(input).toBeChecked();
  await expect(status(page)).toHaveText('10');
  const next = await owned.reply('Automatically appended reply');
  const requests = []; page.on('request', request => { if (new URL(request.url()).pathname === owned.path) requests.push(request); });
  await advance(page, 9); expect(requests).toHaveLength(0); await expect(status(page)).toHaveText('1');
  await advance(page, 1);
  await expect(page.locator(`#p${next}`)).toBeAttached(); await expect(status(page)).toHaveText('10');
  expect(requests).toHaveLength(1);
  await expect(page.locator('#com')).toHaveValue('Retained drafta');
  await expect(page).toHaveTitle(`(1) ${title}`);
  await expect(page.locator(`#p${previous}`)).toHaveClass(/newPostsMarker/);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await advance(page, 0.1);
  await expect(page).toHaveTitle(title); await expect(page.locator('.newPostsMarker')).toHaveCount(0);
  await auto(page).uncheck(); await expect(status(page)).toHaveText('');
  await advance(page, 600); expect(requests).toHaveLength(1);
});

test('idle polling backs off, manual empty updates retain the interval, and visibility cannot add a second loop', async ({ page, owned }) => {
  await initialize(page, owned); await freeze(page); await auto(page).check();
  let requests = 0; page.on('request', request => { if (new URL(request.url()).pathname === owned.path) requests++; });
  await advance(page, 10); await expect(status(page)).toHaveText('15');
  await advance(page, 15); await expect(status(page)).toHaveText('20');
  await advance(page, 1); await manual(page); await expect(status(page)).toHaveText('20');
  expect(requests).toBe(3);
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(status(page)).toHaveText('10');
  let release, intercepted;
  const waiting = new Promise(resolve => { intercepted = resolve; });
  await page.route(`**${owned.path}`, async route => {
    intercepted(); await new Promise(resolve => { release = resolve; });
    await route.fulfill({ status: 503, body: 'Unavailable' }).catch(() => {});
  });
  await advance(page, 10); await waiting;
  await expect(auto(page)).toBeDisabled();
  await page.evaluate(() => { for (let i = 0; i < 40; i++) document.dispatchEvent(new Event('visibilitychange')); });
  await advance(page, 5); expect(requests).toBe(4);
  release(); await expect(status(page)).toHaveText('15'); await expect(auto(page)).toBeEnabled();
  await advance(page, 14); expect(requests).toBe(4);
  await auto(page).uncheck(); await page.unrouteAll({ behavior: 'wait' });
});

test('Auto persists for the current tab and thread, stops explicitly, and always-auto is an initialization default', async ({ page, context, owned }) => {
  await initialize(page, owned);
  await auto(page).check();
  expect(await page.evaluate(id => sessionStorage.getItem(`4chan-auto-${id}`), owned.id)).toBe('1');
  const other = await context.newPage(); await other.goto(owned.url); await expect(auto(other)).not.toBeChecked();
  await page.reload(); await expect(auto(page)).toBeChecked();
  await auto(page).uncheck(); await page.reload(); await expect(auto(page)).not.toBeChecked();
  expect(await page.evaluate(id => sessionStorage.getItem(`4chan-auto-${id}`), owned.id)).toBeNull();
  await other.getByRole('link', { name: 'Settings', exact: true }).click();
  await expect(other.locator('#setting-threadUpdater')).toBeChecked();
  await expect(other.locator('#setting-alwaysAutoUpdate')).not.toBeChecked();
  await other.locator('#setting-alwaysAutoUpdate').check();
  const navigation = other.waitForEvent('load');
  await other.getByRole('button', { name: 'Save Settings', exact: true }).click(); await navigation;
  await expect(auto(other)).toBeChecked();
  await auto(other).uncheck(); await expect(auto(other)).not.toBeChecked();
  await other.reload(); await expect(auto(other)).toBeChecked();
});

test('feature disable cancels an in-flight auto response, restores mobile refresh and resumes once when enabled', async ({ page, context, request, owned }) => {
  await initialize(page, owned, { keyBinds: true }); await freeze(page);
  const other = await context.newPage(); await other.goto(owned.url);
  await auto(page).check();
  const next = await owned.reply('Cancelled automatic response');
  const snapshot = await (await request.get(owned.path)).json();
  let release, intercepted;
  const waiting = new Promise(resolve => { intercepted = resolve; });
  await page.route(`**${owned.path}`, async route => {
    intercepted(); await new Promise(resolve => { release = resolve; });
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }).catch(() => {});
  });
  await advance(page, 10); await waiting;
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true, threadUpdater: false })));
  await expect(auto(page)).toBeHidden();
  await page.setViewportSize({ width: 390, height: 844 });
  const refresh = page.locator('.threadNav.mobile [data-thread-refresh]').first();
  await expect(refresh).toBeVisible(); await expect(refresh).toHaveText('Refresh');
  await expect(refresh).toHaveAttribute('data-updater-ready', 'false');
  release(); await page.unrouteAll({ behavior: 'wait' });
  await advance(page, 100); await expect(page.locator(`#p${next}`)).toHaveCount(0);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true })));
  await expect(page.locator('.threadNav.mobile input[data-cmd="auto"]').first()).toBeChecked();
  await expect(refresh).toHaveText('Update');
  await advance(page, 10); await expect(page.locator(`#p${next}`)).toBeAttached();
});

test('archival stops automatic requests and unavailable session storage still permits in-tab controls', async ({ page, request, owned }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'sessionStorage', { get() { throw new DOMException('unavailable', 'SecurityError'); } });
  });
  await initialize(page, owned); await freeze(page); await auto(page).check();
  const snapshot = await (await request.get(owned.path)).json(); snapshot.archived = true;
  let requests = 0;
  await page.route(`**${owned.path}`, route => { requests++; return route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }); });
  await advance(page, 10); await expect(status(page)).toHaveText('This thread is archived');
  await expect(auto(page)).not.toBeChecked(); await expect(auto(page)).toBeDisabled();
  await advance(page, 600); expect(requests).toBe(1);
});

test('page suspension cancels polling and a persisted pageshow rearms exactly one countdown', async ({ page, owned }) => {
  await initialize(page, owned); await freeze(page); await auto(page).check();
  let requests = 0; page.on('request', request => { if (new URL(request.url()).pathname === owned.path) requests++; });
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await advance(page, 100); expect(requests).toBe(0);
  await page.evaluate(() => {
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
  });
  await expect(status(page)).toHaveText('10'); await advance(page, 10);
  await expect(status(page)).toHaveText('15'); expect(requests).toBe(1);
});

test('the pinned hidden-at-bottom scroll rule follows appended posts without moving a visible reader', async ({ page, owned }) => {
  await owned.reply('Existing reply');
  await page.setViewportSize({ width: 1280, height: 400 });
  await initialize(page, owned, { autoScroll: true }); await freeze(page); await auto(page).check();
  await page.evaluate(() => {
    window.scrollTo(0, document.documentElement.scrollHeight);
    Object.defineProperty(document, 'hidden', { configurable: true, value: true });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  const first = await owned.reply('Hidden-tab append\n'.repeat(15));
  await advance(page, 10); await expect(page.locator(`#p${first}`)).toBeAttached(); await expect(status(page)).toHaveText('60');
  expect(await page.evaluate(() => document.documentElement.scrollHeight === Math.ceil(innerHeight + scrollY))).toBe(true);
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false });
    window.scrollTo(0, 0); document.dispatchEvent(new Event('visibilitychange'));
  });
  const next = await owned.reply('Visible-tab append\n'.repeat(15));
  await advance(page, 10); await expect(page.locator(`#p${next}`)).toBeAttached(); await expect(status(page)).toHaveText('10');
  expect(await page.evaluate(() => scrollY)).toBe(0);
});

test('manual insertion never increments unread and an OP-only auto insertion retains the pinned marker condition', async ({ page, owned }) => {
  await page.setViewportSize({ width: 1280, height: 400 });
  await initialize(page, owned); await freeze(page);
  const title = await page.title(); await auto(page).check();
  await owned.reply('First automatic reply'); await advance(page, 10);
  await expect(status(page)).toHaveText('10'); await expect(page).toHaveTitle(`(1) ${title}`);
  await expect(page.locator('.newPostsMarker')).toHaveCount(0);
  // Static v1191 clears unread only when a last-reply marker exists. This
  // OP-only edge is recorded separately from live-reference qualification.
  await page.evaluate(() => { window.scrollTo(0, document.documentElement.scrollHeight); document.dispatchEvent(new Event('scroll')); });
  await expect(page).toHaveTitle(`(1) ${title}`);
  await advance(page, 1); await owned.reply('Manually fetched reply'); await manual(page);
  await expect(status(page)).toHaveText('10'); await expect(page).toHaveTitle(`(1) ${title}`);
  await auto(page).uncheck(); await expect(page).toHaveTitle(`(1) ${title}`);
});
