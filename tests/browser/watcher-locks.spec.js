import { test as base, expect } from '@playwright/test';
const origin = 'http://127.0.0.1:3000', lockName = 'paperboard-thread-watcher';
const test = base.extend({
  owned: async ({ context }, use) => {
    const request = context.request, password = 'owned-watcher-lock-password';
    const write = form => request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } });
    const response = await write({ resto: '0', sub: 'Owned watcher lock', com: 'Original post' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    try { await use({ id, url: `/demo/thread/${id}`, reply: async (com, track = false) => {
      const response = await write({ resto: id, com, ...(track ? { track: '1' } : {}) });
      expect(response.status()).toBe(303); return response.headers().location.match(/#p(\d+)/)[1];
    } }); }
    finally { await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } }); }
  },
  holder: async ({ context }, use) => {
    const other = await context.newPage(); await other.goto('/settings/theme');
    const hold = async () => {
      await other.evaluate(name => {
        window.lockHeld = false;
        void navigator.locks.request(name, async () => {
          window.lockHeld = true; await new Promise(resolve => { window.releaseHeldLock = resolve; });
        });
      }, lockName);
      await expect.poll(() => other.evaluate(() => window.lockHeld)).toBe(true);
    };
    const release = async () => {
      await other.evaluate(() => window.releaseHeldLock?.());
      await other.evaluate(name => navigator.locks.request(name, () => {}), lockName);
    };
    try { await use({ other, hold, release }); } finally { await release(); await other.close(); }
  },
});
const watch = (page, id) => page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).first();
const unwatch = (page, id) => page.getByRole('button', { name: `Unwatch thread ${id}`, exact: true }).first();
const row = (page, id) => page.locator(`#watch-${id}-demo`);
async function freeze(page) {
  const time = new Date('2026-09-13T20:00:00Z'); await page.clock.install({ time }); await page.clock.pauseAt(time);
}
async function initialize(page, owned) {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true })));
  await page.goto(owned.url); await expect(watch(page, owned.id)).toBeVisible();
  await page.evaluate(name => navigator.locks.request(name, () => {}), lockName);
  await freeze(page);
}
async function queued(holder) {
  await expect.poll(() => holder.other.evaluate(async name => (await navigator.locks.query()).pending.filter(lock => lock.name === name).length, lockName)).toBeGreaterThan(0);
}

test('an expired watch action cannot commit when the actual held lock is released, and a fresh action succeeds', async ({ page, owned, holder }) => {
  await initialize(page, owned); await holder.hold();
  await watch(page, owned.id).click(); await queued(holder);
  await page.clock.runFor(5001);
  await expect(page.locator('.watcherNotice')).toContainText('Watch storage is busy');
  await holder.release(); await expect(row(page, owned.id)).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('4chan-watch'))).toBeNull();
  await watch(page, owned.id).click(); await expect(row(page, owned.id)).toBeVisible();
});

test('a timed-out settings save retains its dialog draft and cannot overwrite storage after lock release', async ({ page, owned, holder }) => {
  await initialize(page, owned);
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Monitoring', exact: true }).click();
  await page.locator('#setting-threadAutoWatcher').check(); await holder.hold();
  const save = page.getByRole('button', { name: 'Save Settings', exact: true }); await save.click(); await queued(holder);
  await expect(save).toBeDisabled(); await page.clock.runFor(5001);
  await expect(save).toBeEnabled(); await expect(page.locator('.settingsMessage')).toContainText('Settings could not be saved');
  await expect(page.locator('#setting-threadAutoWatcher')).toBeChecked();
  await holder.release();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).threadAutoWatcher)).toBeUndefined();
  await page.keyboard.press('Escape');
});

test('page exit cancels queued unwatch and suppression, and persisted resumption permits a fresh action', async ({ page, owned, holder }) => {
  await initialize(page, owned); await watch(page, owned.id).click(); await expect(row(page, owned.id)).toBeVisible();
  await holder.hold(); await unwatch(page, owned.id).click(); await queued(holder);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await holder.release(); await page.clock.runFor(6000);
  await expect(row(page, owned.id)).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('4chan-watch-bl'))).toBeNull();
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await unwatch(page, owned.id).click(); await expect(row(page, owned.id)).toHaveCount(0);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toBeTruthy();
});

test('receipt consumption expires without discarding the real cookie and a later navigation consumes it once', async ({ page, context, owned, holder }) => {
  const reply = await owned.reply('Receipt waiting for storage', true);
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true })));
  await freeze(page); await holder.hold(); await page.goto(owned.url);
  await queued(holder); await page.clock.runFor(10010);
  expect(await page.evaluate(id => localStorage.getItem(`4chan-track-demo-${id}`), owned.id)).toBeNull();
  expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${reply}`)).toBe(true);
  await holder.release();
  expect(await page.evaluate(id => localStorage.getItem(`4chan-track-demo-${id}`), owned.id)).toBeNull();
  await page.reload();
  await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-demo-${id}`) || '{}')[`>>${reply}`], { id: owned.id, reply })).toBe(1);
  expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${reply}`)).toBe(false);
});

test('a watcher-storage failure cannot bypass the held lock for still-available receipt storage', async ({ page, owned, holder }) => {
  const reply = await owned.reply('Tracked through a separate healthy key', true);
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    const get = Storage.prototype.getItem;
    Storage.prototype.getItem = function (key) {
      if (key === '4chan-watch') throw new DOMException('Unavailable watch storage', 'SecurityError');
      return get.call(this, key);
    };
  });
  await freeze(page); await holder.hold(); await page.goto(owned.url); await queued(holder);
  await page.clock.runFor(1000);
  expect(await page.evaluate(id => localStorage.getItem(`4chan-track-demo-${id}`), owned.id)).toBeNull();
  await holder.release();
  await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-demo-${id}`) || '{}')[`>>${reply}`], { id: owned.id, reply })).toBe(1);
});

test('an expired refresh claim does not launch network work after lock release or masquerade as cooldown', async ({ page, owned, holder }) => {
  await initialize(page, owned); await watch(page, owned.id).click(); await expect(row(page, owned.id)).toBeVisible();
  await owned.reply('Unread while refresh storage is busy');
  await holder.hold();
  const requests = []; page.on('request', request => {
    if (new URL(request.url()).pathname.startsWith(`/_watch/demo/thread/${owned.id}`)) requests.push(request.url());
  });
  const refresh = page.getByRole('button', { name: 'Refresh', exact: true });
  await refresh.click(); await queued(holder); await page.clock.runFor(5001);
  await expect(page.locator('.watcherNotice')).toContainText('Watch storage is busy');
  await holder.release(); expect(requests).toEqual([]);
  await refresh.click(); await expect(row(page, owned.id).locator('a')).toHaveClass(/hasNewReplies/);
  expect(requests).toHaveLength(1);
});

test('policy-denied locking makes every shared writer volatile even when local storage remains healthy', async ({ page, context, owned }) => {
  const reply = await owned.reply('Owned receipt with denied locking', true);
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    navigator.locks.request = () => Promise.reject(new DOMException('Denied by browser policy', 'SecurityError'));
  });
  await page.goto(owned.url);
  await expect.poll(async () => (await context.cookies()).some(cookie => cookie.name === `board-posted-${reply}`)).toBe(false);
  await watch(page, owned.id).click(); await expect(row(page, owned.id)).toBeVisible();
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
  const refreshed = page.waitForResponse(response => new URL(response.url()).pathname.startsWith(`/_watch/demo/thread/${owned.id}`));
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  expect((await refreshed).status()).toBe(200);
  await expect(page.locator('.watcherPanel')).toHaveAttribute('aria-busy', 'false');
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Monitoring', exact: true }).click();
  await page.locator('#setting-threadAutoWatcher').check();
  await page.getByRole('button', { name: 'Save Settings', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toHaveCount(0);
  expect(await page.evaluate(id => ({
    watches: localStorage.getItem('4chan-watch'), tracked: localStorage.getItem(`4chan-track-demo-${id}`),
    timestamp: localStorage.getItem('4chan-tw-timestamp'), settings: JSON.parse(localStorage.getItem('4chan-settings')),
  }), owned.id)).toEqual({ watches: null, tracked: null, timestamp: null, settings: { threadWatcher: true } });
});

test('a cookie error inside an acquired lock does not replay consumption or disable healthy persistent writes', async ({ page, owned }) => {
  await owned.reply('Receipt whose deletion is denied', true);
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    const cookie = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie');
    window.receiptDeletionAttempts = 0;
    Object.defineProperty(document, 'cookie', {
      get() { return cookie.get.call(this); },
      set() { window.receiptDeletionAttempts++; throw new DOMException('Cookie deletion denied', 'SecurityError'); },
    });
  });
  await page.goto(owned.url);
  await expect.poll(() => page.evaluate(() => window.receiptDeletionAttempts)).toBe(1);
  await watch(page, owned.id).click(); await expect(row(page, owned.id)).toBeVisible();
  expect(await page.evaluate(id => !!JSON.parse(localStorage.getItem('4chan-watch'))?.[`${id}-demo`], owned.id)).toBe(true);
  expect(await page.evaluate(() => window.receiptDeletionAttempts)).toBe(1);
});
