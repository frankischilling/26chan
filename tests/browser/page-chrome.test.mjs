import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { publicBoardPath } from '../../apps/public/static/page-chrome.v1.js';

const origin = 'https://chrome.example';
const preference = '4chan_never_show_mobile';
const source = await readFile(new URL('../../apps/public/static/page-chrome.v1.js', import.meta.url), 'utf8');
const reference = JSON.parse(await readFile(new URL('../../docs/public-page-chrome-reference.json', import.meta.url)));

test('public board destinations preserve catalog selection and the file-board exception', () => {
  assert.equal(publicBoardPath('demo'), '/demo/');
  assert.equal(publicBoardPath('demo', true), '/demo/catalog');
  assert.equal(publicBoardPath('f', true), '/f/');
  assert.equal(publicBoardPath('0123456789', false), '/0123456789/');
  for (const board of ['', 'Demo', '../demo', 'demo/catalog', 'demo?x=1', 'demo#x', '//foreign.invalid', 'longboard11', '\u0434emo', null, 1]) {
    assert.equal(publicBoardPath(board), null);
  }
  for (const catalog of ['true', 1, null, {}]) assert.equal(publicBoardPath('demo', catalog), null);
});

test('page navigation owns bounded local choices, preferences and its active document', async t => {
  const browser = await chromium.launch();
  async function setup({ catalog = false, stored = null, denyReads = false, denyWrites = false, options = null } = {}) {
    const context = await browser.newContext();
    await context.addInitScript(({ preference, stored, denyReads, denyWrites }) => {
      if (stored !== null) localStorage.setItem(preference, stored);
      localStorage.setItem('owned-unrelated', 'keep');
      if (denyReads) Storage.prototype.getItem = function () { throw new DOMException('Owned unavailable storage', 'SecurityError'); };
      if (denyWrites) for (const method of ['setItem', 'removeItem']) {
        Storage.prototype[method] = function () { throw new DOMException('Owned unavailable storage', 'QuotaExceededError'); };
      }
    }, { preference, stored, denyReads, denyWrites });
    const page = await context.newPage(), navigation = [], rejected = [], errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const choices = options ?? ['demo', 'f', 'zed'];
    await page.route('**/*', route => {
      const url = new URL(route.request().url());
      if (url.origin === origin && url.pathname === '/static/page-chrome.v1.js' && !url.search) {
        return route.fulfill({ contentType: 'text/javascript', body: source });
      }
      if (url.origin === origin && route.request().isNavigationRequest() && /^\/(demo|zed|f)\/(catalog)?$/.test(url.pathname) && !url.search) {
        navigation.push(url.pathname);
        const board = url.pathname.split('/')[1];
        return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><body class="publicPageChrome" data-page-catalog="${url.pathname.endsWith('/catalog')}" data-worksafe="${board !== 'zed'}">
<div id="boardNavDesktop"><a id="desktop-mobile" href="#boardNavMobile" data-page-mobile="enable">Mobile</a></div>
<div id="boardNavMobile"><select id="boardSelectMobile">${choices.map(value => `<option value="${value}"${value === board ? ' selected' : ''}>${value}</option>`).join('')}</select><a id="mobile-desktop" href="#boardNavDesktop" data-page-mobile="disable">Desktop</a></div>
<div id="boardNavDesktopFoot"><a id="footer-mobile" href="#boardNavMobile" data-page-mobile="enable">Mobile</a></div>
<textarea id="draft">Owned draft</textarea><script type="module" src="/static/page-chrome.v1.js"></script></body></html>` });
      }
      if (url.origin === origin && url.pathname === '/favicon.ico') return route.fulfill({ status: 204 });
      rejected.push(url.origin + url.pathname); return route.abort();
    });
    await page.goto(`${origin}/demo/${catalog ? 'catalog' : ''}`);
    await page.evaluate(async () => {
      window.module = await import('/static/page-chrome.v1.js');
      window.chrome = module.mountPageChrome(document.body);
      window.root = document.body; window.select = document.getElementById('boardSelectMobile');
      window.control = document.getElementById('mobile-desktop'); window.draft = document.getElementById('draft');
    });
    return { context, page, navigation, rejected, errors };
  }
  async function close(fixture) {
    assert.deepEqual(fixture.errors, []); assert.deepEqual(fixture.rejected, []);
    await fixture.context.close();
  }
  try {
    for (const row of reference.behavior) await t.test(`${row.mode} selection of ${row.board} matches the public destination`, async () => {
      const fixture = await setup({ catalog: row.mode === 'catalog' });
      try {
        await Promise.all([fixture.page.waitForEvent('load'), fixture.page.selectOption('#boardSelectMobile', row.board)]);
        await fixture.page.waitForURL(origin + row.path);
        assert.deepEqual(fixture.navigation, [`/demo/${row.mode === 'catalog' ? 'catalog' : ''}`, row.path]);
      } finally { await close(fixture); }
    });
    await t.test('only the literal stored true value disables mobile presentation', async () => {
      for (const stored of ['true', 'TRUE', '1', 'false', '"true"', null]) {
        const fixture = await setup({ stored });
        try {
          assert.equal(await fixture.page.locator('body').getAttribute('data-native-never-mobile'), String(stored === 'true'));
          assert.equal(await fixture.page.evaluate(key => localStorage.getItem(key), preference), stored);
        } finally { await close(fixture); }
      }
    });
    await t.test('Desktop and both Mobile controls use one reload and preserve unrelated storage', async () => {
      const fixture = await setup();
      try {
        const { page, navigation } = fixture;
        for (const [selector, expected] of [['#mobile-desktop', 'true'], ['#desktop-mobile', null], ['#mobile-desktop', 'true'], ['#footer-mobile', null]]) {
          const before = navigation.length;
          await Promise.all([page.waitForEvent('load'), page.locator(selector).click()]);
          assert.equal(navigation.length, before + 1);
          assert.equal(await page.evaluate(key => localStorage.getItem(key), preference), expected);
          assert.equal(await page.evaluate(() => localStorage.getItem('owned-unrelated')), 'keep');
          assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), String(expected === 'true'));
        }
      } finally { await close(fixture); }
    });
    await t.test('blocked reads retain mobile defaults and blocked writes report failure without navigation', async () => {
      for (const denied of [{ denyReads: true }, { denyWrites: true }]) {
        const fixture = await setup(denied);
        try {
          const { page, navigation } = fixture;
          assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'false');
          if (denied.denyWrites) {
            await page.locator('#mobile-desktop').click(); await page.locator('#mobile-desktop').click();
            assert.equal(await page.getByRole('status').count(), 1);
            assert.equal(await page.getByRole('status').textContent(), 'The mobile preference could not be saved.');
            assert.equal(navigation.length, 1);
            assert.equal(await page.evaluate(() => draft === document.getElementById('draft') && draft.value === 'Owned draft'), true);
          }
        } finally { await close(fixture); }
      }
    });
    await t.test('new options and detached controls cannot authorize a navigation', async () => {
      const fixture = await setup();
      try {
        await fixture.page.evaluate(() => {
          const option = document.createElement('option'); option.value = 'unknown'; select.append(option); select.value = 'unknown';
          select.dispatchEvent(new Event('change'));
          select.remove(); select.value = 'zed'; select.dispatchEvent(new Event('change'));
          control.remove(); control.dispatchEvent(new MouseEvent('click', { button: 0, bubbles: true, cancelable: true }));
        });
        assert.equal(fixture.navigation.length, 1);
        assert.equal(await fixture.page.evaluate(key => localStorage.getItem(key), preference), null);
      } finally { await close(fixture); }
    });
    await t.test('malformed or oversized server choices leave the select inert', async () => {
      for (const options of [['demo', 'bad-board'], Array(101).fill('demo')]) {
        const fixture = await setup({ options });
        try {
          await fixture.page.evaluate(() => select.dispatchEvent(new Event('change')));
          assert.equal(fixture.navigation.length, 1);
        } finally { await close(fixture); }
      }
    });
    await t.test('modifier clicks preserve native link behavior without changing storage', async () => {
      const fixture = await setup();
      try {
        assert.deepEqual(await fixture.page.evaluate(() => ['ctrlKey', 'metaKey', 'shiftKey', 'altKey'].map(modifier => {
          const event = new MouseEvent('click', { button: 0, cancelable: true, [modifier]: true });
          control.dispatchEvent(event); return event.defaultPrevented;
        })), [false, false, false, false]);
        assert.equal(await fixture.page.evaluate(key => localStorage.getItem(key), preference), null);
        assert.equal(fixture.navigation.length, 1);
      } finally { await close(fixture); }
    });
    await t.test('suspended, replaced and retired documents ignore stale events', async () => {
      const fixture = await setup();
      try {
        const { page } = fixture;
        await page.evaluate(key => {
          dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          localStorage.setItem(key, 'true'); dispatchEvent(new StorageEvent('storage', { key }));
          select.value = 'zed'; select.dispatchEvent(new Event('change'));
        }, preference);
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'false');
        assert.equal(fixture.navigation.length, 1);
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'true');
        await page.evaluate(key => {
          const replacement = document.createElement('body'); replacement.textContent = 'Owned replacement'; root.replaceWith(replacement);
          localStorage.removeItem(key); dispatchEvent(new StorageEvent('storage', { key }));
          control.dispatchEvent(new MouseEvent('click', { button: 0, cancelable: true }));
          select.dispatchEvent(new Event('change')); chrome.destroy();
          dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        }, preference);
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), null);
        assert.equal(fixture.navigation.length, 1);
      } finally { await close(fixture); }
    });
    await t.test('repeated mounting retires the prior owner and cleanup preserves another writer', async () => {
      const fixture = await setup();
      try {
        const { page } = fixture;
        await page.evaluate(key => {
          window.first = chrome; chrome = module.mountPageChrome(root); first.destroy();
          localStorage.setItem(key, 'true'); dispatchEvent(new StorageEvent('storage', { key }));
        }, preference);
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'true');
        await page.evaluate(() => { root.setAttribute('data-native-never-mobile', 'another-owner'); chrome.destroy(); });
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'another-owner');
        await page.evaluate(key => { localStorage.removeItem(key); dispatchEvent(new StorageEvent('storage', { key })); }, preference);
        assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'another-owner');
        assert.equal(fixture.navigation.length, 1);
      } finally { await close(fixture); }
    });
  } finally { await browser.close(); }
});
