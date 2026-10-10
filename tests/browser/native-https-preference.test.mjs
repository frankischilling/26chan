import assert from 'node:assert/strict';
import { test, after } from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { chromium } from '@playwright/test';
import { configuredHTTPSOrigin, httpsPreferenceEnabled, settingsHTTPSRedirect, httpsPreferenceCookie,
  settingAvailable } from '../../apps/public/static/native-settings.v1.js';
import { buildSettingsTransfer, parseSettingsTransferHash } from '../../apps/public/static/native-settings-transfer.v1.js';

const reference = JSON.parse(await readFile(new URL('../fixtures/native-https-preference-source.json', import.meta.url)));
const snippets = Object.fromEntries(Object.entries(reference.snippets).map(([key, row]) => [key, row.text]));
const http = 'http://127.0.0.1:3000', https = 'https://127.0.0.1:3443';

function source(raw, cookie, href = `${http}/demo/?page=2#p123`) {
  const trace = [], cookies = [];
  const sandbox = { Config: { forceHTTPS: false, darkTheme: false }, Main: {
    getCookie: () => cookie, setCookie: (...args) => cookies.push(['set', ...args]),
    removeCookie: (...args) => cookies.push(['remove', ...args]), run() {},
  }, localStorage: { getItem: () => raw, setItem: (...args) => trace.push(['write', ...args]) },
  $: { extend: Object.assign }, document: { addEventListener() {} }, Date,
  location: { href, protocol: new URL(href).protocol, host: new URL(href).host },
  UA: { init: () => trace.push(['ua']) },
  };
  vm.createContext(sandbox);
  vm.runInContext(snippets.load + '\n' + snippets.save + '\n' + snippets.init, sandbox, { timeout: 100 });
  return { sandbox, trace, cookies };
}

for (const raw of [null, '', '{}', '{"forceHTTPS":false}', '{"forceHTTPS":true}', '{"disableAll":true}']) {
  for (const cookie of [null, '0', '1', '11']) test(`source cookie authority: ${raw}, cookie=${cookie}`, () => {
    const s = source(raw, cookie); s.sandbox.Config.load();
    const actualCookie = cookie === null ? '' : `https=${cookie}`;
    assert.equal(httpsPreferenceEnabled(raw, actualCookie), s.sandbox.Config.forceHTTPS);
    assert.equal(settingsHTTPSRedirect(https, `${http}/demo/?page=2#p123`, raw, actualCookie),
      s.sandbox.Config.forceHTTPS ? `${https}/demo/?page=2#p123` : null);
  });
}

test('source init redirects before imports and disabled-extension parsing', () => {
  const s = source('{"disableAll":true}', '1'); s.sandbox.Main.init();
  assert.equal(s.sandbox.location.href, `${http.replace('http:', 'https:')}/demo/?page=2#p123`);
  assert.deepEqual(s.trace, [['ua']]);
  assert.equal(settingsHTTPSRedirect(https, `${http}/demo/?page=2#p123`, '{"disableAll":true}', 'https=1'),
    `${https}/demo/?page=2#p123`);
});

test('source explicit save updates the cookie; first-open and import save do not', () => {
  for (const enabled of [false, true]) for (const explicit of [false, true]) {
    const s = source('{}', null); s.sandbox.Config.forceHTTPS = enabled;
    s.sandbox.Config.save(explicit ? { darkTheme: false } : undefined);
    assert.equal(s.trace.length, 1);
    assert.deepEqual(s.cookies, explicit ? enabled ? [['set', 'https', 1]] : [['remove', 'https']] : []);
    assert.match(httpsPreferenceCookie(enabled), enabled ? /^https=1;.*Max-Age=31536000/ : /^https=;.*Max-Age=0/);
    assert.doesNotMatch(httpsPreferenceCookie(enabled), /Domain=|Secure|HttpOnly/);
  }
});

test('only a fixed HTTPS endpoint on the current hostname can select a navigation', () => {
  assert.equal(configuredHTTPSOrigin(https, `${http}/demo/`), https);
  assert.equal(settingsHTTPSRedirect(https, `${https}/demo/`, '{}', 'https=1'), null);
  for (const origin of [undefined, '', http, 'https://other.example', `${https}/path`, `${https}?x=1`,
    `${https}#x`, 'https://user@127.0.0.1:3443', 'javascript:alert(1)', 'https:' + 'x'.repeat(2049)]) {
    assert.equal(configuredHTTPSOrigin(origin, `${http}/demo/`), null, String(origin));
    assert.equal(settingsHTTPSRedirect(origin, `${http}/demo/`, '{}', 'https=1'), null);
  }
  assert.equal(settingsHTTPSRedirect(https, `${http}//evil.example/%2Fdemo?x=//evil#p1`, '{}', 'https=1'),
    `${https}//evil.example/%2Fdemo?x=//evil#p1`);
  for (const href of ['file:///demo', 'http://user@127.0.0.1:3000/', 'http://other.example/demo', 'x'.repeat(65537)]) {
    assert.equal(configuredHTTPSOrigin(https, href), null);
  }
});

test('malformed settings, oversized cookies and unrelated cookie names cannot enable HTTPS', () => {
  for (const raw of ['{', 'false', 'null', '[]', '{"forceHTTPS":"true"}', '{"constructor":true}', 'x'.repeat(4097)]) {
    assert.equal(httpsPreferenceEnabled(raw, 'https=1'), false);
  }
  for (const cookie of ['', 'xhttps=1', 'https=11', 'https=0; https=1', 'https=1' + 'x'.repeat(16384), null]) {
    assert.equal(httpsPreferenceEnabled('{}', cookie), false);
  }
  for (const cookie of ['https=1', 'a=2; https=1; b=3', 'https=1; https=0']) {
    assert.equal(httpsPreferenceEnabled('{}', cookie), true);
  }
});

test('desktop and mobile availability and transfer review reflect the configured endpoint', () => {
  const raw = '{"forceHTTPS":true,"unmuteWebm":true}';
  for (const available of [false, true]) {
    for (const mobile of [false, true]) assert.equal(settingAvailable('forceHTTPS', mobile, available), available);
    const transfer = buildSettingsTransfer(key => key === '4chan-settings' ? raw : null, { httpsAvailable: available });
    assert.equal(transfer.payload.settings, raw);
    assert.deepEqual(transfer.inactiveCompatibility, available ? ['unmuteWebm'] : ['forceHTTPS', 'unmuteWebm']);
    const review = parseSettingsTransferHash(`#cfg=${transfer.encoded}`, { httpsAvailable: available });
    assert.equal(review.review.settings.find(row => row.key === 'forceHTTPS').inactiveCompatibility, !available);
  }
});

let browser;
after(async () => { await browser?.close(); });
async function fixture({ raw = '{}', cookie = null, endpoint = https, mobile = false, unavailable = false, secure = false } = {}) {
  browser ??= await chromium.launch();
  const context = await browser.newContext({ viewport: { width: mobile ? 390 : 1000, height: 800 } });
  if (cookie !== null) await context.addCookies([{ name: 'https', value: cookie, url: http }]);
  await context.addInitScript(({ raw, unavailable, secure }) => {
    if (location.protocol === (secure ? 'https:' : 'http:')) {
      if (raw !== null) localStorage.setItem('4chan-settings', raw);
      if (unavailable) Storage.prototype.getItem = () => { throw new DOMException('Unavailable', 'SecurityError'); };
    }
    window.initCount = 0; document.addEventListener('4chanMainInit', () => initCount++);
  }, { raw, unavailable, secure });
  const navigations = [], errors = [], requests = [];
  await context.route('**/*', async route => {
    const url = new URL(route.request().url()); requests.push(url.href);
    if (![http, https].includes(url.origin)) return route.abort();
    if (route.request().isNavigationRequest()) {
      navigations.push(url.href);
      if (url.protocol === 'https:' && !secure) return route.fulfill({ contentType: 'text/html', body: '<title>HTTPS destination</title>' });
      return route.fulfill({ contentType: 'text/html', body: `<!doctype html><meta charset="utf-8">
        <div class="boardList"></div><div class="board"></div>
        <div id="watcher-context" hidden data-board="demo" data-thread="0" data-catalog="false" data-public-origin="${endpoint}"></div>
        <script type="module" src="/static/thread-watcher.v1.js"></script>` });
    }
    if (/^\/static\/[a-z0-9-]+(?:\.v1)?\.js$/.test(url.pathname)) {
      return route.fulfill({ contentType: 'text/javascript', body: await readFile(new URL(`../../apps/public${url.pathname}`, import.meta.url)) });
    }
    return route.abort();
  });
  const page = await context.newPage(); page.on('pageerror', error => errors.push(error.message));
  await page.goto(`${secure ? https : http}/demo/?page=2#p123`);
  return { page, context, navigations, errors, requests };
}

test('integrated startup redirects a disabled extension before MainInit and preserves URL components', async () => {
  const h = await fixture({ raw: '{"disableAll":true}', cookie: '1' });
  try {
    await h.page.waitForURL(`${https}/demo/?page=2#p123`);
    assert.equal(await h.page.title(), 'HTTPS destination');
    assert.equal(await h.page.evaluate(() => initCount), 0);
    assert.deepEqual(h.navigations, [`${http}/demo/?page=2`, `${https}/demo/?page=2`]);
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});

for (const mobile of [false, true]) test(`integrated explicit save and cancel use the host cookie (mobile=${mobile})`, async () => {
  const h = await fixture({ mobile });
  try {
    await h.page.locator(mobile ? '#settingsWindowLinkMobile' : '#settingsWindowLink').click();
    const category = h.page.getByRole('button', { name: 'Miscellaneous', exact: true }); await category.click();
    const setting = h.page.locator('#setting-forceHTTPS'); assert.equal(await setting.isChecked(), false);
    await setting.check(); await h.page.locator('#settings-close').click();
    assert.equal((await h.context.cookies()).some(row => row.name === 'https'), false);
    await h.page.locator(mobile ? '#settingsWindowLinkMobile' : '#settingsWindowLink').click(); await category.click();
    assert.equal(await setting.isChecked(), false);
    await setting.check(); await h.page.locator('#settings-save').click();
    await h.page.waitForURL(`${https}/demo/?page=2`);
    assert.equal((await h.context.cookies()).find(row => row.name === 'https').value, '1');
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});

for (const [name, options] of [
  ['HTTP-only endpoint', { endpoint: http, cookie: '1' }],
  ['foreign endpoint', { endpoint: 'https://other.example', cookie: '1' }],
  ['fresh storage', { raw: null, cookie: '1' }],
  ['unavailable storage', { unavailable: true, cookie: '1' }],
  ['transferred boolean without cookie', { raw: '{"forceHTTPS":true}' }],
]) test(`integrated startup does not navigate for ${name}`, async () => {
  const h = await fixture(options);
  try {
    await h.page.waitForFunction(() => document.querySelector('[data-native-settings-ready]'));
    assert.equal(h.page.url(), `${http}/demo/?page=2#p123`);
    assert.equal(await h.page.evaluate(() => initCount), 1);
    if (options.endpoint) {
      await h.page.locator('#settingsWindowLink').click();
      assert.equal(await h.page.locator('#setting-forceHTTPS').count(), 0);
    }
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});

test('HTTPS save removes the host cookie and stays on HTTPS after reload', async () => {
  const h = await fixture({ secure: true, raw: '{"forceHTTPS":false}', cookie: '1' });
  try {
    await h.page.locator('#settingsWindowLink').click();
    await h.page.getByRole('button', { name: 'Miscellaneous', exact: true }).click();
    assert.equal(await h.page.locator('#setting-forceHTTPS').isChecked(), true);
    await h.page.locator('#setting-forceHTTPS').uncheck();
    await h.page.locator('#settings-save').click();
    await h.page.waitForURL(`${https}/demo/?page=2`);
    assert.equal((await h.context.cookies()).some(row => row.name === 'https'), false);
    await h.page.locator('#settingsWindowLink').click();
    assert.equal(await h.page.locator('#setting-forceHTTPS').isChecked(), false);
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});

test('fresh Settings reads observe a cookie changed by another tab without overwriting it on cancel', async () => {
  const h = await fixture({ secure: true });
  try {
    await h.page.locator('#settingsWindowLink').click();
    assert.equal(await h.page.locator('#setting-forceHTTPS').isChecked(), false);
    await h.page.locator('#settings-close').click();
    await h.context.addCookies([{ name: 'https', value: '1', url: http }]);
    await h.page.locator('#settingsWindowLink').click();
    assert.equal(await h.page.locator('#setting-forceHTTPS').isChecked(), true);
    await h.page.locator('#settings-close').click();
    assert.equal((await h.context.cookies()).find(row => row.name === 'https').value, '1');
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});

test('a denied cookie write reports the partial save without navigating or claiming HTTPS enabled', async () => {
  const h = await fixture();
  try {
    await h.page.locator('#settingsWindowLink').click();
    await h.page.getByRole('button', { name: 'Miscellaneous', exact: true }).click();
    await h.page.locator('#setting-forceHTTPS').check();
    await h.page.evaluate(() => {
      Object.defineProperty(document, 'cookie', { get: () => '', set: () => { throw new DOMException('Blocked', 'SecurityError'); } });
    });
    await h.page.locator('#settings-save').click();
    await h.page.locator('.settingsMessage').filter({ hasText: 'browser cookies are unavailable' }).waitFor();
    assert.equal(h.page.url(), `${http}/demo/?page=2#p123`);
    assert.equal((await h.context.cookies()).some(row => row.name === 'https'), false);
    assert.equal(await h.page.locator('#settingsMenu').count(), 1);
    assert.deepEqual(h.errors, []);
  } finally { await h.context.close(); }
});
