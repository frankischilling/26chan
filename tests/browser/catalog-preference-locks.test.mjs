import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const origin = 'https://catalog-locks.example';
const lockName = 'paperboard-thread-watcher';
const initial = JSON.stringify({ orderby: 'alt', large: false, extended: true });
const restored = JSON.stringify({ orderby: 'date', large: true, extended: true });

function card(id, text, replies = 1) {
  return `<section class="thread" data-thread-id="${id}" data-bumped="${10 - id}" data-latest-reply="${id}"
    data-replies="${replies}" data-sticky="false"><a class="catalogThumb" href="/test/thread/${id}"
    data-search-text="${text}" data-search-file="" data-has-file="false"><span class="thumb">${text}</span></a>
    <div class="teaser">${text} teaser</div><div class="meta"><b data-replies-count>${replies}</b></div></section>`;
}

function html() {
  return `<!doctype html><html><body><form id="ctrl" action="/test/catalog" method="get">
    <select id="order-ctrl" name="order"><option value="alt">Bump</option><option value="absdate">Last reply</option><option value="date">Creation</option><option value="r">Replies</option></select>
    <select id="size-ctrl" name="size"><option value="small">Small</option><option value="large">Large</option></select>
    <select id="teaser-ctrl" name="teaser"><option value="on">On</option><option value="off">Off</option></select>
    <input id="qf-box" type="search" name="q"><button type="submit">Apply</button><a id="catalog-reset" href="/test/catalog">Reset</a>
    <small id="catalog-preference-status" role="status" hidden></small></form>
    <div id="threads" class="catalog extended-small" data-threads-per-page="2">${card(1, 'alpha')}${card(2, 'beta')}</div>
    <template id="catalogFiltered"></template><script type="module" src="/static/catalog-preferences.v1.js"></script></body></html>`;
}

test('catalog preference mutations share the watcher lock and remain race-safe', async t => {
  const files = {
    '/static/catalog-preferences.v1.js': await readFile(new URL('../../apps/public/static/catalog-preferences.v1.js', import.meta.url), 'utf8'),
    '/static/native-filter.v1.js': await readFile(new URL('../../apps/public/static/native-filter.v1.js', import.meta.url), 'utf8'),
  };
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(options = {}) {
      const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
      await context.addInitScript(({ initial, lockName, options }) => {
        if (location.origin !== 'https://catalog-locks.example') return;
        const getItem = Storage.prototype.getItem;
        const setItem = Storage.prototype.setItem;
        const removeItem = Storage.prototype.removeItem;
        window.__catalogRaw = () => getItem.call(localStorage, 'catalog-settings');
        window.__catalogSetRaw = raw => raw === null
          ? removeItem.call(localStorage, 'catalog-settings') : setItem.call(localStorage, 'catalog-settings', raw);
        if (getItem.call(localStorage, 'catalog-settings') === null) setItem.call(localStorage, 'catalog-settings', initial);
        if (options.pinAndHide) {
          setItem.call(localStorage, '4chan-pin-test', JSON.stringify({ 1: 1 }));
          setItem.call(localStorage, '4chan-hide-t-test', JSON.stringify({ 2: true }));
        }
        if (options.fastLockTimeout) {
          const later = window.setTimeout.bind(window);
          window.setTimeout = (fn, ms, ...args) => later(fn, ms === 5000 ? 40 : ms, ...args);
        }
        if (options.denyStorage) {
          Storage.prototype.getItem = function (key) {
            if (key === 'catalog-settings') throw new DOMException('Test storage denial', 'SecurityError');
            return getItem.call(this, key);
          };
          Storage.prototype.setItem = function (key, value) {
            if (key === 'catalog-settings') throw new DOMException('Test storage denial', 'SecurityError');
            return setItem.call(this, key, value);
          };
          Storage.prototype.removeItem = function (key) {
            if (key === 'catalog-settings') throw new DOMException('Test storage denial', 'SecurityError');
            return removeItem.call(this, key);
          };
        }
        if (options.denyLock && navigator.locks) {
          const request = navigator.locks.request.bind(navigator.locks);
          Object.defineProperty(navigator.locks, 'request', { configurable: true, value(name, ...args) {
            if (name === lockName) throw new DOMException('Test lock denial', 'SecurityError');
            return request(name, ...args);
          } });
        }
      }, { initial, lockName, options });
      await context.route('**/*', route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && files[url.pathname]) return route.fulfill({ contentType: 'text/javascript', body: files[url.pathname] });
        if (url.origin === origin && url.pathname === '/test/catalog') return route.fulfill({ contentType: 'text/html', body: html() });
        if (url.origin === origin && url.pathname === '/blank') return route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>lock holder</title>' });
        if (url.pathname.endsWith('favicon.ico')) return route.fulfill({ status: 204 });
        return route.abort();
      });
      const page = await context.newPage(), errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.goto(`${origin}/test/catalog`);
      await page.waitForFunction(() => document.querySelector('#catalog-unpin-all'));
      return { context, page, errors };
    }

    async function raw(page) { return page.evaluate(() => window.__catalogRaw()); }
    async function hold(context) {
      const page = await context.newPage();
      await page.goto(`${origin}/blank`);
      await page.evaluate(name => new Promise(resolve => {
        window.__heldDone = navigator.locks.request(name, async () => {
          resolve();
          await new Promise(release => { window.__releaseHeld = release; });
          if (window.__heldRaw !== undefined) localStorage.setItem('catalog-settings', window.__heldRaw);
        });
      }), lockName);
      return page;
    }
    async function release(page, rawValue) {
      await page.evaluate(async value => {
        if (value !== undefined) window.__heldRaw = value;
        window.__releaseHeld();
        await window.__heldDone;
      }, rawValue);
    }

    await t.test('a held cross-tab lock preserves immediate display and only the latest local action commits', async () => {
      const { context, page, errors } = await setup();
      const other = await hold(context);
      try {
        await page.locator('#order-ctrl').selectOption('r');
        await page.locator('#size-ctrl').selectOption('large');
        await page.locator('#teaser-ctrl').selectOption('off');
        assert.equal(await page.locator('#order-ctrl').inputValue(), 'r');
        assert.equal(await page.locator('#threads').getAttribute('class'), 'catalog large');
        assert.equal(await raw(page), initial);
        assert.equal(await page.locator('#catalog-preference-status').isHidden(), true);
        await release(other);
        await page.waitForFunction(() => localStorage.getItem('catalog-settings') === JSON.stringify({ orderby: 'r', large: true, extended: false }));
        assert.deepEqual(JSON.parse(await raw(page)), { orderby: 'r', large: true, extended: false });
        assert.equal(await page.locator('#catalog-preference-status').isHidden(), true);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('Reset applies defaults immediately but removes catalog-settings only after the shared lock', async () => {
      const { context, page, errors } = await setup();
      const saved = JSON.stringify({ orderby: 'r', large: true, extended: false });
      await page.evaluate(rawValue => {
        window.__catalogSetRaw(rawValue);
        document.dispatchEvent(new Event('4chanPreferencesRestored'));
      }, saved);
      await page.waitForFunction(() => document.querySelector('#order-ctrl').value === 'r');
      const other = await hold(context);
      try {
        await page.getByRole('link', { name: 'Reset', exact: true }).click();
        assert.equal(await page.locator('#order-ctrl').inputValue(), 'alt');
        assert.equal(await page.locator('#size-ctrl').inputValue(), 'small');
        assert.equal(await page.locator('#teaser-ctrl').inputValue(), 'on');
        assert.equal(await raw(page), saved);
        await release(other);
        await page.waitForFunction(() => localStorage.getItem('catalog-settings') === null);
        assert.equal(await raw(page), null);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('CAS rejects a stale queued choice after restored storage changes without a storage event', async () => {
      const { context, page, errors } = await setup();
      const other = await hold(context);
      try {
        await page.locator('#order-ctrl').selectOption('r');
        await page.evaluate(rawValue => window.__catalogSetRaw(rawValue), restored);
        await release(other);
        await page.waitForFunction(value => localStorage.getItem('catalog-settings') === value, restored);
        await page.waitForFunction(() => document.querySelector('#catalog-preference-status').textContent.includes('changed before'));
        assert.equal(await raw(page), restored);
        assert.equal(await page.locator('#order-ctrl').inputValue(), 'r');
        assert.match(await page.locator('#catalog-preference-status').textContent(), /changed before/);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('cross-tab storage cancels a stale save, while the restore event applies in place without storage writes', async () => {
      const { context, page, errors } = await setup({ pinAndHide: true });
      const other = await hold(context);
      let navigations = 0;
      page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations++; });
      try {
        await page.locator('#qf-box').fill('alpha');
        await page.waitForTimeout(300);
        await page.locator('#order-ctrl').selectOption('r');
        await release(other, restored);
        await page.waitForFunction(value => localStorage.getItem('catalog-settings') === value, restored);
        await page.waitForFunction(() => document.querySelector('#catalog-preference-status').textContent.includes('another tab'));
        assert.equal(await page.locator('#order-ctrl').inputValue(), 'r');
        assert.equal(await page.locator('#qf-box').inputValue(), 'alpha');
        assert.equal(await page.locator('[data-thread-id="2"]').count(), 0);
        assert.equal(await page.locator('[data-thread-id="1"] .thumb.pinned').count(), 1);

        await page.evaluate(() => {
          const write = Storage.prototype.setItem;
          window.__restoreWrites = [];
          Storage.prototype.setItem = function (key, value) { window.__restoreWrites.push(key); return write.call(this, key, value); };
          document.dispatchEvent(new Event('4chanPreferencesRestored'));
        });
        await page.waitForFunction(() => document.querySelector('#order-ctrl').value === 'date');
        assert.equal(await page.locator('#size-ctrl').inputValue(), 'large');
        assert.equal(await page.locator('#qf-box').inputValue(), 'alpha');
        assert.equal(await page.locator('[data-thread-id="2"]').count(), 0);
        assert.equal(await page.locator('[data-thread-id="1"] .thumb.pinned').count(), 1);
        assert.deepEqual(await page.evaluate(() => window.__restoreWrites), []);
        assert.equal(await raw(page), restored);
        const url = new URL(page.url());
        assert.equal(url.searchParams.get('q'), 'alpha');
        assert.equal(url.searchParams.get('order'), 'date');
        assert.equal(navigations, 0);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('persisted pagehide aborts queued work and pageshow resumes fresh writes', async () => {
      const { context, page, errors } = await setup();
      const other = await hold(context);
      try {
        await page.locator('#order-ctrl').selectOption('r');
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
        await release(other);
        await page.waitForTimeout(50);
        assert.equal(await raw(page), initial);
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        await page.locator('#size-ctrl').selectOption('large');
        await page.waitForFunction(() => localStorage.getItem('catalog-settings') === JSON.stringify({ orderby: 'r', large: true, extended: true }));
        assert.deepEqual(JSON.parse(await raw(page)), { orderby: 'r', large: true, extended: true });
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('a detached catalog root cannot commit after acquiring the queued lock', async () => {
      const { context, page, errors } = await setup();
      const other = await hold(context);
      try {
        await page.locator('#order-ctrl').selectOption('r');
        await page.evaluate(() => document.querySelector('#threads').remove());
        await release(other);
        await page.waitForTimeout(50);
        assert.equal(await raw(page), initial);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('busy, storage-denied and lock-denied persistence stays non-fatal and visible', async () => {
      {
        const { context, page, errors } = await setup({ fastLockTimeout: true });
        const other = await hold(context);
        try {
          await page.locator('#order-ctrl').selectOption('r');
          await page.waitForFunction(() => document.querySelector('#catalog-preference-status').textContent.includes('busy'));
          assert.equal(await raw(page), initial);
          assert.equal(await page.locator('#order-ctrl').inputValue(), 'r');
          await release(other);
          assert.deepEqual(errors, []);
        } finally { await context.close(); }
      }
      for (const options of [{ denyStorage: true }, { denyLock: true }]) {
        const { context, page, errors } = await setup(options);
        try {
          await page.locator('#order-ctrl').selectOption('r');
          await page.waitForFunction(() => document.querySelector('#catalog-preference-status').textContent.includes('unavailable'));
          assert.equal(await page.locator('#order-ctrl').inputValue(), 'r');
          assert.equal(await raw(page), initial);
          assert.equal(await page.locator('#catalog-preference-status').isHidden(), false);
          assert.deepEqual(errors, []);
        } finally { await context.close(); }
      }
    });
  } finally { await browser.close(); }
});
