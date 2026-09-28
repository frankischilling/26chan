import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const source = await readFile(new URL('../../apps/public/static/native-thread-stats.v1.js', import.meta.url), 'utf8');
let browser;
before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });

const active = {
  version: 1, board: 'demo', thread: '123', replies: 12, images: 5,
  sticky: true, closed: true, archived: false, bump_limited: true,
  image_limited: false, page: 3,
};

async function fixture(t, { useRealTransport = false } = {}) {
  const context = await browser.newContext();
  t.after(() => context.close());
  const page = await context.newPage();
  await page.route('**/*', route => {
    if (route.request().isNavigationRequest()) return route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Owned stats fixture</title>' });
    return route.abort();
  });
  await page.goto('https://boards.test/demo/thread/123');
  await page.evaluate(async ({ source, active, useRealTransport }) => {
    const moduleUrl = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    const stats = await import(moduleUrl);
    URL.revokeObjectURL(moduleUrl);
    window.statsModule = stats;
    document.body.innerHTML = `
      <nav class="threadNav mobile" data-watch-position="top-mobile"></nav>
      <nav class="threadNav desktop" data-watch-position="top-desktop"></nav>
      <main class="board"><section class="thread" id="t123"></section></main>
      <div id="bottom"><nav class="threadNav desktop" data-watch-position="bottom-desktop"></nav><nav class="threadNav mobile" data-watch-position="bottom-mobile"></nav></div>`;
    window.statsConfig = {};
    window.neverMobile = null;
    window.timerHandles = new Map();
    window.timerDelays = [];
    let nextTimer = 1;
    window.fakeLater = (action, delay) => {
      const id = nextTimer++;
      timerHandles.set(id, action); timerDelays.push(delay);
      return id;
    };
    window.fakeClear = id => timerHandles.delete(id);
    window.mobile = {
      matches: false, listeners: new Set(),
      addEventListener(type, listener) { if (type === 'change') this.listeners.add(listener); },
      removeEventListener(type, listener) { if (type === 'change') this.listeners.delete(listener); },
      set(value) { this.matches = value; for (const listener of this.listeners) listener({ matches: value }); },
    };
    if (useRealTransport) {
      window.rawResponses = [JSON.stringify(active)];
      const response = (raw, target) => {
        const bytes = new TextEncoder().encode(raw);
        return { url: target, status: 200, redirected: false,
          headers: new Headers({ 'content-type': 'application/json', 'content-length': String(bytes.length) }),
          body: new ReadableStream({ start(controller) { controller.enqueue(bytes); controller.close(); } }) };
      };
      window.fetchCount = 0;
      window.statsTransport = new stats.NativeThreadStatsTransport({ origin: location.origin, board: 'demo', thread: '123',
        fetcher: async url => { fetchCount++; return response(rawResponses.shift() ?? '{', url); } });
    } else {
      window.statsTransport = {
        loads: 0, cancels: 0, responses: [{ status: 'ok', snapshot: active }], pending: [],
        load({ signal } = {}) {
          this.loads++;
          if (this.responses.length) return Promise.resolve(this.responses.shift());
          return new Promise(resolve => {
            const entry = { resolve };
            this.pending.push(entry);
            signal?.addEventListener('abort', () => {
              this.pending = this.pending.filter(item => item !== entry);
              resolve({ status: 'cancelled' });
            }, { once: true });
          });
        },
        cancel() {
          this.cancels++;
          for (const entry of this.pending.splice(0)) entry.resolve({ status: 'cancelled' });
        },
        push(result) {
          const entry = this.pending.shift();
          if (entry) entry.resolve(result); else this.responses.push(result);
        },
      };
    }
    window.statsController = stats.mountNativeThreadStats({ board: 'demo', thread: '123',
      settings: () => statsConfig, mobile, readNeverMobile: () => neverMobile,
      transport: statsTransport, later: fakeLater, clear: fakeClear });
  }, { source, active, useRealTransport });
  await page.waitForFunction(() => document.querySelectorAll('.thread-stats').length === 2);
  return page;
}

test('desktop and mobile placement render status, bounded counts, limit emphasis and no poster identity', async t => {
  const page = await fixture(t);
  const desktop = await page.evaluate(() => ({
    count: document.querySelectorAll('.thread-stats').length,
    topParent: document.querySelector('.thread-stats')?.parentElement?.dataset.watchPosition,
    bottomParent: document.querySelectorAll('.thread-stats')[1]?.parentElement?.dataset.watchPosition,
    text: document.querySelector('.thread-stats')?.textContent,
    repliesTag: document.querySelector('.ts-replies')?.tagName,
    repliesTip: document.querySelector('.ts-replies')?.dataset.tip,
    imagesTag: document.querySelector('.ts-images')?.tagName,
    page: document.querySelector('.ts-page')?.textContent,
    posters: document.querySelectorAll('.ts-ips,[data-tip="Posters"]').length,
    poll: timerDelays.at(-1),
    timers: timerHandles.size,
  }));
  assert.deepEqual(desktop, {
    count: 2, topParent: 'top-desktop', bottomParent: 'bottom-desktop',
    text: 'Sticky / Closed / 12 / 5 / 3', repliesTag: 'EM',
    repliesTip: 'Replies (bump limit reached)', imagesTag: 'SPAN', page: '3', posters: 0,
    poll: 180000, timers: 1,
  });

  await page.evaluate(() => mobile.set(true));
  const mobile = await page.evaluate(() => {
    const node = document.querySelector('.thread-stats');
    return { count: document.querySelectorAll('.thread-stats').length,
      previous: node?.previousElementSibling?.dataset.watchPosition,
      parent: node?.parentElement?.id };
  });
  assert.deepEqual(mobile, { count: 1, previous: 'bottom-mobile', parent: 'bottom' });

  await page.evaluate(() => {
    neverMobile = 'true';
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  assert.equal(await page.locator('.threadNav.desktop .thread-stats').count(), 2);
});

test('accepted updates replace coherent stats and archived state removes page and stops polling', async t => {
  const page = await fixture(t);
  const archived = { ...active, replies: 20, images: 9, sticky: false, closed: true, archived: true,
    bump_limited: false, image_limited: true, page: null };
  await page.evaluate(archived => {
    statsTransport.push({ status: 'ok', snapshot: archived });
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  }, archived);
  await page.waitForFunction(() => document.querySelector('.thread-stats')?.textContent.includes('Archived'));
  const state = await page.evaluate(() => ({
    text: document.querySelector('.thread-stats').textContent,
    imagesTag: document.querySelector('.ts-images').tagName,
    imagesTip: document.querySelector('.ts-images').dataset.tip,
    page: document.querySelectorAll('.ts-page').length,
    timers: timerHandles.size,
    snapshot: statsController.snapshot(),
  }));
  assert.equal(state.text, 'Archived / 20 / 9');
  assert.equal(state.imagesTag, 'EM');
  assert.equal(state.imagesTip, 'Images (limit reached)');
  assert.equal(state.page, 0);
  assert.equal(state.timers, 0);
  assert.deepEqual(state.snapshot, archived);
  const loads = await page.evaluate(() => statsTransport.loads);
  await page.evaluate(() => document.dispatchEvent(new Event('boardThreadStateChanged')));
  await page.waitForTimeout(0);
  assert.equal(await page.evaluate(() => statsTransport.loads), loads);
});

test('a missing live thread retires polling without erasing the last coherent display', async t => {
  const page = await fixture(t);
  const original = await page.locator('.thread-stats').first().textContent();
  await page.evaluate(() => {
    statsTransport.push({ status: 'http-error', httpStatus: 404 });
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  });
  await page.waitForFunction(() => timerHandles.size === 0);
  assert.equal(await page.locator('.thread-stats').first().textContent(), original);
  const loads = await page.evaluate(() => statsTransport.loads);
  await page.evaluate(() => document.dispatchEvent(new Event('boardThreadStateChanged')));
  await page.waitForTimeout(0);
  assert.equal(await page.evaluate(() => statsTransport.loads), loads);
});

test('default-on settings, disableAll, suspension, offline state and teardown cancel work without duplicate polling', async t => {
  const page = await fixture(t);
  const initialLoads = await page.evaluate(() => statsTransport.loads);
  await page.evaluate(() => {
    statsConfig = { threadStats: false };
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  });
  assert.equal(await page.locator('.thread-stats').count(), 0);
  assert.equal(await page.evaluate(() => timerHandles.size), 0);

  await page.evaluate(active => {
    statsConfig = {};
    statsTransport.push({ status: 'ok', snapshot: active });
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  }, active);
  await page.waitForFunction(() => document.querySelectorAll('.thread-stats').length === 2);
  assert.ok(await page.evaluate(() => statsTransport.loads > 1));

  await page.evaluate(() => {
    statsConfig = { threadStats: true, disableAll: true };
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  });
  assert.equal(await page.locator('.thread-stats').count(), 0);
  await page.evaluate(active => {
    statsConfig = { threadStats: true };
    statsTransport.push({ status: 'ok', snapshot: active });
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  }, active);
  await page.waitForFunction(() => document.querySelectorAll('.thread-stats').length === 2);

  await page.evaluate(() => {
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  });
  await page.waitForFunction(() => statsTransport.pending.length === 1);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.equal(await page.evaluate(() => statsTransport.pending.length), 0);
  assert.equal(await page.evaluate(() => timerHandles.size), 0);
  await page.evaluate(active => {
    statsTransport.push({ status: 'ok', snapshot: active });
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
  }, active);
  await page.waitForFunction(() => timerHandles.size === 1);

  await page.evaluate(() => {
    Object.defineProperty(navigator, 'onLine', { configurable: true, value: false });
    window.dispatchEvent(new Event('offline'));
  });
  assert.equal(await page.evaluate(() => timerHandles.size), 0);
  await page.evaluate(active => {
    Object.defineProperty(navigator, 'onLine', { configurable: true, value: true });
    statsTransport.push({ status: 'ok', snapshot: active });
    window.dispatchEvent(new Event('online'));
  }, active);
  await page.waitForFunction(() => timerHandles.size === 1);

  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  assert.equal(await page.evaluate(() => timerHandles.size), 0);
  await page.evaluate(active => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false });
    statsTransport.push({ status: 'ok', snapshot: active });
    document.dispatchEvent(new Event('visibilitychange'));
  }, active);
  await page.waitForFunction(() => timerHandles.size === 1);

  const before = await page.evaluate(() => statsTransport.loads);
  await page.evaluate(() => statsController.disconnect());
  assert.equal(await page.locator('.thread-stats').count(), 0);
  assert.equal(await page.evaluate(() => timerHandles.size), 0);
  await page.evaluate(() => {
    document.dispatchEvent(new Event('boardThreadStateChanged'));
    document.dispatchEvent(new Event('4chanSettingsSaved'));
    window.dispatchEvent(new Event('online'));
  });
  assert.equal(await page.evaluate(() => statsTransport.loads), before);
  assert.ok(before > initialLoads);
});

test('strict main-thread transport keeps extra identity fields and partial bodies out of the DOM', async t => {
  const page = await fixture(t, { useRealTransport: true });
  const original = await page.locator('.thread-stats').first().textContent();
  assert.equal(original, 'Sticky / Closed / 12 / 5 / 3');
  await page.evaluate(active => {
    rawResponses.push(JSON.stringify({ ...active, replies: 99, unique_ips: 11 }));
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  }, active);
  await page.waitForFunction(() => fetchCount >= 2);
  await page.waitForTimeout(0);
  assert.equal(await page.locator('.thread-stats').first().textContent(), original);
  assert.equal(await page.locator('.ts-ips,[data-tip="Posters"]').count(), 0);

  await page.evaluate(() => {
    rawResponses.push('{"version":1,"board":"demo","thread":"123","replies":500');
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  });
  await page.waitForFunction(() => fetchCount >= 3);
  await page.waitForTimeout(0);
  assert.equal(await page.locator('.thread-stats').first().textContent(), original);
  assert.equal(await page.locator('.thread-stats').count(), 2);
});

test('cancelled requests that ignore abort cannot overwrite a newer enabled snapshot', async t => {
  const page = await fixture(t);
  await page.evaluate(() => {
    window.late = [];
    statsTransport.load = () => new Promise(resolve => late.push(resolve));
    statsTransport.cancel = () => {};
    void statsController.refresh();
  });
  await page.waitForFunction(() => late.length === 1);
  await page.evaluate(() => {
    statsConfig.threadStats = false;
    document.dispatchEvent(new Event('4chanSettingsSaved'));
    statsConfig.threadStats = true;
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  });
  await page.waitForFunction(() => late.length === 2);
  await page.evaluate(active => late[1]({ status: 'ok', snapshot: { ...active, replies: 23 } }), active);
  await page.waitForFunction(() => document.querySelector('.ts-replies').textContent === '23');
  await page.evaluate(async active => { late[0]({ status: 'ok', snapshot: { ...active, replies: 99 } }); await Promise.resolve(); }, active);
  assert.equal(await page.locator('.ts-replies').first().textContent(), '23');
  assert.equal(await page.evaluate(() => statsController.snapshot().replies), 23);
  assert.equal(await page.evaluate(() => timerHandles.size), 1);
});
