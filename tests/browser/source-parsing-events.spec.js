import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';

async function listen(page) {
  await page.addInitScript(() => {
    window.parsingEvents = [];
    window.mainEvents = [];
    document.addEventListener('4chanMainInit', event => mainEvents.push({
      constructor: event.constructor.name, bubbles: event.bubbles, cancelable: event.cancelable,
      target: event.target === document, detail: Object.hasOwn(event, 'detail'),
      board: document.getElementById('watcher-context').dataset.board,
      parsed: parsingEvents.length, menus: document.querySelectorAll('[data-post-menu]').length,
    }));
    document.addEventListener('4chanParsingDone', event => {
      const section = document.getElementById(`t${event.detail.threadId}`);
      parsingEvents.push({ mainBefore: mainEvents.length, detail: event.detail, constructor: event.constructor.name,
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
  let primaryError;
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
    expect(await page.evaluate(() => mainEvents)).toEqual([{ constructor: 'Event', bubbles: false,
      cancelable: false, target: true, detail: false, board: 'demo', parsed: 0, menus: 0 }]);
    const update = page.locator('.nativeUpdater [data-cmd="update"]:visible').first();
    if (deferred) {
      await expect(update).toHaveCount(1);
      reply = await write({ resto: id, com: 'Owned dynamic event reply' }); expect(reply.status()).toBe(303);
      await update.click({ timeout: 5_000 });
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
    const initial = { mainBefore: 1, detail: { threadId: id, offset: 0, limit: 1 }, constructor: 'Event',
      bubbles: false, cancelable: false, target: true, count: 1, menus: 1 };
    expect(await page.evaluate(() => parsingEvents)).toEqual([initial]);
    reply ??= await write({ resto: id, com: 'Owned dynamic event reply' }); expect(reply.status()).toBe(303);
    await update.click({ timeout: 5_000 });
    await expect.poll(() => page.evaluate(() => parsingEvents.length)).toBe(3);
    expect(await page.evaluate(() => parsingEvents)).toEqual([initial,
      { ...initial, detail: { threadId: id, offset: 1, limit: 2 }, count: 2, menus: 2 }, { updated: true }]);
    await page.evaluate(() => {
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    });
    await page.waitForTimeout(100);
    expect(await page.evaluate(() => parsingEvents.length)).toBe(3);
    expect(await page.evaluate(() => mainEvents.length)).toBe(1);
    await page.goto('/demo/');
    if (deferred) await page.evaluate(() => releaseStartupReceipts());
    expect(await page.evaluate(() => mainEvents.length)).toBe(1);
    await expect.poll(() => page.evaluate(id => parsingEvents.filter(e => e.detail?.threadId === id).length, id)).toBe(1);
    expect(await page.evaluate(id => parsingEvents.find(e => e.detail?.threadId === id).detail, id))
      .toEqual({ threadId: id, offset: 0, limit: 2 });
  } catch (error) {
    primaryError = error;
    throw error;
  } finally {
    try {
      await withDeletionQuota(async () => {
        const removed = await request.post('/demo/delete', { headers: { Origin: 'http://127.0.0.1:3000' },
          maxRedirects: 0, timeout: 5_000, form: { no: id, password } });
        expect(removed.status()).toBe(303);
      });
    } catch (cleanupError) {
      if (primaryError) throw new AggregateError([primaryError, cleanupError], 'Source event test and cleanup both failed');
      throw cleanupError;
    }
  }
});
