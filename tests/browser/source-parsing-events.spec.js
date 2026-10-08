import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';

async function listen(page) {
  await page.addInitScript(() => {
    window.parsingEvents = [];
    document.addEventListener('4chanParsingDone', event => {
      const section = document.getElementById(`t${event.detail.threadId}`);
      parsingEvents.push({ detail: event.detail, constructor: event.constructor.name,
        bubbles: event.bubbles, cancelable: event.cancelable, target: event.target === document,
        count: section.querySelectorAll(':scope > .postContainer').length,
        menus: section.querySelectorAll('[data-post-menu]').length });
    });
    document.addEventListener('4chanThreadUpdated', () => parsingEvents.push({ updated: true }));
  });
}

for (const deferred of [false, true, 'bfcache']) test(`listeners registered before real bootstrap see ordered ranges (deferred receipts: ${deferred})`, async ({ page, request }) => {
  const password = 'owned-source-events';
  const write = form => withPostingHistory(() => request.post('/demo/post', {
    headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0, form: { ...form, password },
  }));
  const response = await write({ resto: '0', sub: 'Source events', com: 'Owned event OP' });
  expect(response.status()).toBe(303);
  const id = response.headers().location.match(/thread\/(\d+)/)[1];
  try {
    await listen(page);
    let reply, fetches = 0;
    page.on('request', request => { if (request.url().includes(`/_watch/demo/thread/${id}/posts`)) fetches++; });
    if (deferred) await page.addInitScript(() => {
      const request = navigator.locks.request.bind(navigator.locks);
      const hold = new Promise(resolve => { window.releaseStartupReceipts = resolve; });
      navigator.locks.request = (...args) => args[0] === 'paperboard-thread-watcher'
        ? hold.then(() => request(...args)) : request(...args);
    });
    await page.goto(`/demo/thread/${id}`);
    if (deferred) {
      await expect(page.locator('.nativeUpdater [data-cmd="update"]').first()).toHaveCount(1);
      reply = await write({ resto: id, com: 'Owned dynamic event reply' }); expect(reply.status()).toBe(303);
      await page.locator('.nativeUpdater [data-cmd="update"]').first().evaluate(link => link.click());
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      expect(fetches).toBe(0);
      expect(await page.evaluate(() => parsingEvents)).toEqual([]);
      if (deferred === 'bfcache') {
        await page.evaluate(() => {
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        expect(await page.evaluate(() => parsingEvents)).toEqual([]);
      }
      await page.evaluate(() => releaseStartupReceipts());
    }
    await expect.poll(() => page.evaluate(() => parsingEvents.length)).toBe(1);
    const initial = { detail: { threadId: id, offset: 0, limit: 1 }, constructor: 'Event',
      bubbles: false, cancelable: false, target: true, count: 1, menus: 1 };
    expect(await page.evaluate(() => parsingEvents)).toEqual([initial]);
    reply ??= await write({ resto: id, com: 'Owned dynamic event reply' }); expect(reply.status()).toBe(303);
    await page.locator('.nativeUpdater [data-cmd="update"]').first().click();
    await expect.poll(() => page.evaluate(() => parsingEvents.length)).toBe(3);
    expect(await page.evaluate(() => parsingEvents)).toEqual([initial,
      { ...initial, detail: { threadId: id, offset: 1, limit: 2 }, count: 2, menus: 2 }, { updated: true }]);
    await page.evaluate(() => {
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    });
    await page.waitForTimeout(100);
    expect(await page.evaluate(() => parsingEvents.length)).toBe(3);
    await page.goto('/demo/');
    if (deferred) await page.evaluate(() => releaseStartupReceipts());
    await expect.poll(() => page.evaluate(id => parsingEvents.filter(e => e.detail?.threadId === id).length, id)).toBe(1);
    expect(await page.evaluate(id => parsingEvents.find(e => e.detail?.threadId === id).detail, id))
      .toEqual({ threadId: id, offset: 0, limit: 2 });
  } finally {
    await withDeletionQuota(async () => {
      const removed = await request.post('/demo/delete', { headers: { Origin: 'http://127.0.0.1:3000' },
        maxRedirects: 0, form: { no: id, password } });
      expect(removed.status()).toBe(303);
    });
  }
});
