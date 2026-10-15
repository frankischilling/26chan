import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { chromium } from '@playwright/test';
const source = await readFile(new URL('../../apps/public/static/page-chrome.v1.js', import.meta.url), 'utf8');
const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8');
const reference = JSON.parse(await readFile(new URL('../fixtures/global-announcement-source.json', import.meta.url)));
const origin = 'https://announcement.example';
async function fixture(t) {
  const browser = await chromium.launch(); t.after(() => browser.close());
  const page = await browser.newPage(); const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname === '/static/page-chrome.v1.js') return route.fulfill({ contentType: 'text/javascript', body: source });
    if (url.pathname === '/static/board.css') return route.fulfill({ contentType: 'text/css', body: css });
    if (/^\/static\/watcher\/futaba\/post_expand_(plus|minus)(@2x)?\.png$/.test(url.pathname)) {
      return readFile(new URL(`../../apps/public${url.pathname}`, import.meta.url)).then(body => route.fulfill({ contentType: 'image/png', body }));
    }
    if (url.pathname === '/') return route.fulfill({ contentType: 'text/html', body: '<!doctype html><link rel="stylesheet" href="/static/board.css"><body class="publicPageChrome" data-page-catalog="false"></body>' });
    return route.fulfill({ status: 404 });
  });
  await page.goto(origin);
  await page.evaluate(async () => { window.announcementModule = await import('/static/page-chrome.v1.js'); });
  t.after(() => assert.deepEqual(errors, []));
  return page;
}
async function setup(page, { current = '1700000000', previous = null, mode = 'board', mobile = false, disabled = false, denied = false, empty = false } = {}) {
  await page.setViewportSize({ width: mobile ? 390 : 1280, height: 800 });
  await page.evaluate(options => {
    window.announcement?.destroy(); window.chrome?.destroy();
    document.body.innerHTML = '<a id="globalToggle" class="button redButton mobile hidden" href="#globalMessage" aria-expanded="false">View Important Announcement</a><div id="globalMessage" class="globalMessage hideMobile"></div><hr>';
    document.body.dataset.pageCatalog = String(options.mode === 'catalog');
    const message = document.getElementById('globalMessage'); message.dataset.utc = options.current;
    message.textContent = options.empty ? '' : 'Owned <b>plain text</b> announcement';
    if (options.previous === null) localStorage.removeItem('4chan-global-msg');
    else localStorage.setItem('4chan-global-msg', options.previous);
    window.announcementSettings = { disableAll: options.disabled };
    window.chrome = announcementModule.mountPageChrome(document.body);
    const get = Storage.prototype.getItem, set = Storage.prototype.setItem, remove = Storage.prototype.removeItem;
    if (options.denied) for (const key of ['getItem', 'setItem', 'removeItem']) Storage.prototype[key] = () => { throw new DOMException('Unavailable', 'SecurityError'); };
    window.announcement = announcementModule.mountDesktopAnnouncement(document.body,
      { settings: () => announcementSettings, mobileLayout: options.mobile, catalog: options.mode === 'catalog' });
    window.restoreStorage = () => { Storage.prototype.getItem = get; Storage.prototype.setItem = set; Storage.prototype.removeItem = remove; };
  }, { current, previous, mode, mobile, disabled, denied, empty });
}
const state = page => page.evaluate(() => ({ hidden: document.getElementById('globalMessage').style.display === 'none', stored: localStorage.getItem('4chan-global-msg') }));

test('desktop and catalog controls match every original dismissal and re-open vector', async t => {
  const page = await fixture(t);
  for (const row of reference.cases) {
    await setup(page, row);
    assert.deepEqual(await state(page), row.states[0]);
    for (const expected of row.states.slice(1)) {
      await page.locator('#toggleMsgBtn').click(); assert.deepEqual(await state(page), expected);
    }
    assert.equal(await page.locator('#globalMessage b').count(), 0);
  }
});
test('mobile core toggle works with extension disabled and does not change desktop dismissal', async t => {
  const page = await fixture(t);
  await setup(page, { mobile: true, disabled: true, previous: '1700000000' });
  assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
  assert.equal(await page.locator('#globalMessage').isVisible(), false);
  for (const expected of reference.mobile) {
    await page.locator('#globalToggle').click();
    assert.equal(await page.locator('#globalMessage').isVisible(), !expected.hidden);
    assert.equal(await page.locator('#globalToggle').textContent(), expected.label);
    assert.equal(await page.evaluate(() => localStorage.getItem('4chan-global-msg')), '1700000000');
  }
});
test('catalog dismissal is independent of disableAll; unavailable storage and empty content stay usable', async t => {
  const page = await fixture(t);
  await setup(page, { disabled: true }); assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
  await setup(page, { disabled: true, mode: 'catalog' }); assert.equal(await page.locator('#toggleMsgBtn').count(), 1);
  await setup(page, { mode: 'catalog', denied: true }); assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
  await page.evaluate(() => restoreStorage());
  await setup(page, { denied: true }); await page.locator('#toggleMsgBtn').click();
  assert.equal(await page.locator('#globalMessage').isVisible(), false);
  await page.evaluate(() => restoreStorage());
  await setup(page, { empty: true }); assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
});
test('announcement controllers retain ownership through settings, BFCache and disposal', async t => {
  const page = await fixture(t); await setup(page);
  await page.locator('#toggleMsgBtn').click();
  await page.evaluate(() => {
    announcementSettings.disableAll = true; document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
  assert.equal(await page.locator('#globalMessage').isVisible(), true);
  await page.evaluate(() => { announcementSettings.disableAll = false; document.dispatchEvent(new CustomEvent('4chanSettingsSaved')); });
  assert.equal(await page.locator('#globalMessage').isVisible(), false);
  await page.evaluate(() => {
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    document.getElementById('toggleMsgBtn').click();
  });
  assert.equal((await state(page)).hidden, true);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await page.locator('#toggleMsgBtn').click(); assert.equal((await state(page)).hidden, false);
  await page.evaluate(() => { announcement.destroy(); announcement.destroy(); chrome.destroy(); });
  assert.equal(await page.locator('#toggleMsgBtn').count(), 0);
  assert.equal(await page.locator('#globalMessage').isVisible(), true);
});
