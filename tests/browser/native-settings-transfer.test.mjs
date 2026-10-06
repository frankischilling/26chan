import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import {
  SETTINGS_TRANSFER_LIMITS,
  SETTINGS_TRANSFER_STORAGE_KEYS,
  buildSettingsTransfer,
  canonicalBoardURL,
  checkTransferValues,
  mountNativeSettingsTransfer,
  parseSettingsTransferHash,
  settingsTransferURL,
  validateCatalogSettings,
  validateTransferCSS,
  validateTransferFilters,
  validateTransferCatalogFilters,
  validateTransferSettings,
} from '../../apps/public/static/native-settings-transfer.v1.js';
import { quickReplyPosition } from '../../apps/public/client/native-quick-reply-position.js';

const settingsRaw = JSON.stringify({
  quotePreview: true,
  customCSS: true,
  customMenu: true,
  customMenuList: 'demo test',
  'TW-position': 'left: 10px; top: 380px;',
  'TN-position': 'left: 10%; top: 50px;',
  'SN-position': 'right: 10px; top: 50px; position: fixed;',
  'QR-position': { left: 20, top: 30 },
});
const filterRaw = JSON.stringify([{
  type: 2, pattern: 'needle', boards: 'demo', active: true, auto: false, hide: true,
}]);
const cssRaw = '.reply { color: #112233; padding-left: 8px; }';
const catalogRaw = JSON.stringify({ orderby: 'r', large: true, extended: false });
const catalogFiltersRaw = JSON.stringify({ 0: { active: 1, pattern: 'needle', boards: 'demo', hidden: 1, top: 0 } });
const publicDefaultSettingsRaw = JSON.stringify({
  quotePreview: true, backlinks: true, quickReply: true, threadUpdater: true, threadHiding: true,
  alwaysAutoUpdate: false, topPageNav: false, threadWatcher: false, threadAutoWatcher: false,
  imageExpansion: true, fitToScreenExpansion: false, threadExpansion: true, alwaysDepage: false,
  localTime: true, stickyNav: false, keyBinds: false, inlineQuotes: false, filter: false,
  revealSpoilers: false, imageHover: false, threadStats: true, IDColor: true, noPictures: false,
  embedYouTube: true, embedSoundCloud: false, updaterSound: false, customCSS: false, autoScroll: false,
  hideStubs: false, compactThreads: false, centeredThreads: false, dropDownNav: false, autoHideNav: false,
  classicNav: false, fixedThreadWatcher: false, persistentQR: false, forceHTTPS: false, darkTheme: false,
  linkify: false, unmuteWebm: false, disableAll: false,
});

const payloadHash = payload => `#cfg=${encodeURIComponent(JSON.stringify(payload))}`;

test('current settings, filters, CSS and catalog formats validate without widening storage authority', () => {
  assert.equal(validateTransferSettings(settingsRaw).status, 'ok');
  assert.equal(validateTransferSettings(publicDefaultSettingsRaw).status, 'ok');
  assert.equal(validateTransferFilters(filterRaw).status, 'ok');
  assert.equal(validateTransferCSS(cssRaw).status, 'ok');
  assert.equal(validateCatalogSettings(catalogRaw).status, 'ok');
  assert.deepEqual(SETTINGS_TRANSFER_STORAGE_KEYS,
    ['4chan-settings', '4chan-filters', '4chan-css', 'catalog-filters', 'catalog-settings']);

  for (const raw of [
    '{"constructor":false}',
    '{"quotePreview":"true"}',
    '{"unknownPreference":true}',
    JSON.stringify({ customMenuList: 'x '.repeat(600) }),
    '{"TW-position":"position: fixed;"}',
    '{"QR-position":{"left":1,"top":2,"extra":3}}',
  ]) assert.equal(validateTransferSettings(raw).status, 'invalid', raw);
  assert.equal(validateTransferSettings('{"__proto__":{"polluted":true}}').status, 'invalid');
  assert.equal(validateTransferFilters(JSON.stringify([{ type: 2, pattern: 'x', boards: 'demo', active: true, constructor: {} }])).status, 'invalid');
  assert.equal(validateTransferCSS('.reply { display: none; }').status, 'invalid');
  assert.equal(validateCatalogSettings('{"orderby":"r","large":true,"extended":false,"extra":true}').status, 'invalid');
});

test('public v1191 default settings export preserves inactive compatibility booleans without enabling them', () => {
  const transfer = buildSettingsTransfer(key => key === '4chan-settings' ? publicDefaultSettingsRaw : null);
  assert.equal(transfer.status, 'ok');
  assert.equal(transfer.payload.settings, publicDefaultSettingsRaw);
  assert.deepEqual(transfer.inactiveCompatibility.sort(), ['forceHTTPS', 'unmuteWebm']);
  const parsed = parseSettingsTransferHash(`#cfg=${transfer.encoded}`);
  assert.equal(parsed.status, 'ok');
  assert.deepEqual(parsed.review.settings.filter(setting => setting.inactiveCompatibility).map(setting => setting.key).sort(),
    ['forceHTTPS', 'unmuteWebm']);
});

test('source Quick Reply coordinates restore within the current viewport without interpreting arbitrary CSS', () => {
  const geometry = { width: 1000, height: 800, panelWidth: 300, panelHeight: 200 };
  for (const [value, expected] of [
    ['right: 20px; top: 10%;', { left: 680, top: 80 }],
    ['left: 25%; bottom: 10%; position: fixed;', { left: 250, top: 520 }],
    ['left: 9000%; top: 1000000px;', { left: 700, top: 600 }],
    [{ left: -10, top: 30 }, { left: 0, top: 30 }],
  ]) {
    assert.equal(validateTransferSettings(JSON.stringify({ 'QR-position': value })).status, 'ok');
    assert.deepEqual(quickReplyPosition(value, geometry), expected);
  }
  for (const value of ['left: 1px; top: 2px; display:none;', 'right: 0; left: 1px; top: 0;',
    'left: var(--x); top: 0;', 'left: 1px; top: 0; background:url(/attack);']) {
    assert.equal(validateTransferSettings(JSON.stringify({ 'QR-position': value })).status, 'invalid');
    assert.equal(quickReplyPosition(value, geometry), null);
  }
  assert.equal(quickReplyPosition('left: 10px; top: 10px;', { ...geometry, width: Infinity }), null);
});

test('transaction revalidation accepts only supported validated raw storage values', () => {
  assert.deepEqual(checkTransferValues({
    '4chan-settings': settingsRaw,
    '4chan-filters': filterRaw,
    '4chan-css': cssRaw,
    'catalog-settings': catalogRaw,
  }), {
    status: 'ok',
    values: {
      '4chan-settings': settingsRaw,
      '4chan-filters': filterRaw,
      '4chan-css': cssRaw,
      'catalog-settings': catalogRaw,
    },
  });
  for (const values of [
    {},
    { '4chan-settings': settingsRaw, '4chan-watch': '{}' },
    { '4chan-settings': settingsRaw, '4chan-css': '' },
    { '4chan-settings': { quotePreview: true } },
  ]) assert.equal(checkTransferValues(values).status, 'invalid');
});

test('restore links are bounded before decoding and nested parsing', () => {
  assert.equal(parseSettingsTransferHash('#other=x').status, 'none');
  assert.equal(parseSettingsTransferHash(`#cfg=${'x'.repeat(SETTINGS_TRANSFER_LIMITS.encodedChars + 1)}`).error,
    'The restore link is too large.');
  assert.equal(parseSettingsTransferHash(`#cfg=${'a'.repeat(SETTINGS_TRANSFER_LIMITS.decodedChars + 1)}`).error,
    'The restore payload is too large.');
  assert.equal(parseSettingsTransferHash('#cfg=%E0%A4%A').error, 'The restore link is not correctly encoded.');
  assert.equal(parseSettingsTransferHash(payloadHash({ settings: settingsRaw, catalogFilters: '[]' })).status,
    'invalid');
  assert.equal(parseSettingsTransferHash(payloadHash({ settings: settingsRaw, css: '.reply { display: none; }' })).status, 'invalid');
  assert.equal(parseSettingsTransferHash(payloadHash({ settings: '{"prototype":true}' })).status, 'invalid');
  assert.equal(parseSettingsTransferHash(payloadHash({ settings: settingsRaw, filters: '[{"__proto__":{}}]' })).status, 'invalid');

  const parsed = parseSettingsTransferHash(payloadHash({
    settings: settingsRaw, filters: filterRaw, css: cssRaw, catalogSettings: catalogRaw,
  }));
  assert.equal(parsed.status, 'ok');
  assert.deepEqual(parsed.values, {
    '4chan-settings': settingsRaw,
    '4chan-filters': filterRaw,
    '4chan-css': cssRaw,
    'catalog-settings': catalogRaw,
  });
});

test('export reads only preference keys and builds a canonical same-origin board link', () => {
  const reads = [];
  const stored = new Map([
    ['4chan-settings', settingsRaw], ['4chan-filters', filterRaw], ['4chan-css', cssRaw],
    ['catalog-settings', catalogRaw], ['catalog-filters', catalogFiltersRaw],
    ['4chan-watch', '{"secret":"watch"}'], ['4chan-post-receipts', '{"secret":"receipt"}'], ['password', 'secret'],
  ]);
  const readItem = key => { reads.push(key); return stored.get(key) ?? null; };
  const built = buildSettingsTransfer(readItem);
  assert.equal(built.status, 'ok');
  assert.deepEqual(reads, SETTINGS_TRANSFER_STORAGE_KEYS);
  assert.equal(built.payload.catalogFilters, catalogFiltersRaw);
  assert.equal(JSON.stringify(built.payload).includes('watch'), false);
  assert.equal(JSON.stringify(built.payload).includes('receipt'), false);
  assert.equal(JSON.stringify(built.payload).includes('secret'), false);

  assert.equal(canonicalBoardURL('https://boards.example/demo/thread/123?x=1#p123'), 'https://boards.example/demo/');
  assert.equal(canonicalBoardURL('https://boards.example/demo/upload?resto=123'), 'https://boards.example/demo/');
  assert.equal(canonicalBoardURL('javascript:alert(1)'), null);
  const transfer = settingsTransferURL('https://boards.example/demo/thread/123?x=1', key => stored.get(key) ?? null);
  assert.equal(transfer.status, 'ok');
  assert.ok(transfer.url.startsWith('https://boards.example/demo/#cfg='));
  assert.equal(parseSettingsTransferHash(new URL(transfer.url).hash).status, 'ok');
});

test('catalog transfer preserves independently replayed raw fields and separates the two rule formats', async () => {
  const reference = JSON.parse(await readFile(new URL('../../docs/public-settings-transfer-reference.json', import.meta.url)));
  for (const example of reference.exports) {
    const built = buildSettingsTransfer(key => key === '4chan-settings' ? example.payload.settings : example.stored[key] ?? null);
    assert.equal(built.status, 'ok', example.name);
    assert.deepEqual(built.payload, example.payload, example.name);
    const restored = parseSettingsTransferHash('#cfg=' + built.encoded);
    assert.equal(restored.status, 'ok');
    if (Object.hasOwn(example.payload, 'catalogFilters')) {
      assert.equal(restored.values['catalog-filters'], example.payload.catalogFilters);
      assert.equal(restored.review.catalogFilters.raw, example.payload.catalogFilters);
    }
  }
  assert.equal(validateTransferCatalogFilters(filterRaw).status, 'invalid');
  assert.equal(validateTransferFilters(catalogFiltersRaw).status, 'invalid');
  assert.equal(validateTransferCatalogFilters('{}').count, 0);
});

test('catalog transfer bounds, flags and reserved nested keys reject the complete payload', () => {
  const row = { active: 1, pattern: 'paper', boards: '', hidden: 0, top: 0 };
  const valid = raw => validateTransferCatalogFilters(raw).status;
  assert.equal(valid(JSON.stringify(Object.fromEntries(Array.from({ length: 64 }, (_, i) => [i, row])))), 'ok');
  const bad = [null, '[]', 'null', '', '{', ' '.repeat(SETTINGS_TRANSFER_LIMITS.catalogFiltersChars + 1),
    JSON.stringify(Object.fromEntries(Array.from({ length: 65 }, (_, i) => [i, row]))),
    JSON.stringify({ 100000: row }), JSON.stringify({ '01': row }),
    JSON.stringify({ 0: { ...row, pattern: 'x'.repeat(1025) } }),
    JSON.stringify({ 0: { ...row, boards: 'x'.repeat(1025) } }),
    JSON.stringify({ 0: { ...row, active: '1' } }),
    '{"0":{"active":1,"pattern":"paper","boards":"","extra":{"constructor":{}}}}',
  ];
  for (const raw of bad) {
    assert.equal(valid(raw), 'invalid');
    assert.equal(parseSettingsTransferHash('#cfg=' + encodeURIComponent(JSON.stringify({ settings: '{}', catalogFilters: raw }))).status, 'invalid');
  }
  const booleans = JSON.stringify({ 7: { ...row, active: true, hidden: false, top: true } });
  assert.deepEqual(checkTransferValues({ '4chan-settings': '{}', 'catalog-filters': booleans }), {
    status: 'ok', values: { '4chan-settings': '{}', 'catalog-filters': booleans },
  });
});

const sourceNames = [
  'native-settings-transfer.v1.js', 'native-filter.v1.js', 'native-custom-css.v1.js',
  'native-display.v1.js', 'watcher-position.v1.js',
  'catalog-filter-core.v1.js',
];
const sources = Object.fromEntries(await Promise.all(sourceNames.map(async name => [name,
  await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8')])));
const origin = 'https://settings-transfer.example';
let browser;

after(async () => { await browser?.close(); });

async function fixture(t, { path = '/demo/thread/100', hash = '', stored = {}, restoreMode = 'ok' } = {}) {
  // Pure transfer validation must not depend on a browser executable being installed.
  browser ??= await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  t.after(() => context.close());
  const requests = [];
  await context.route('**/*', async route => {
    const url = new URL(route.request().url());
    const name = url.pathname.startsWith('/static/') ? url.pathname.slice('/static/'.length) : null;
    if (url.origin === origin && name && Object.hasOwn(sources, name)) {
      return route.fulfill({ contentType: 'text/javascript', body: sources[name] });
    }
    if (url.origin === origin && url.pathname === '/favicon.ico') return route.fulfill({ status: 204 });
    if (route.request().isNavigationRequest() && url.origin === origin) {
      return route.fulfill({
        contentType: 'text/html',
        headers: { 'content-security-policy': "default-src 'none'; script-src 'self'; style-src 'self'" },
        body: '<!doctype html><html><head><meta charset="utf-8"></head><body><button id="opener" type="button">Open</button><main class="board"></main></body></html>',
      });
    }
    requests.push(url.href); return route.abort();
  });
  const page = await context.newPage();
  await page.goto(`${origin}${path}${hash}`);
  await page.evaluate(async ({ stored, restoreMode }) => {
    window.transferAPI = await import('/static/native-settings-transfer.v1.js');
    window.transferStore = new Map(Object.entries(stored));
    window.restoreMode = restoreMode;
    window.restoreCalls = [];
    window.restoredEvents = [];
    document.addEventListener('4chanPreferencesRestored', event => restoredEvents.push(event.detail));
    window.readItem = key => transferStore.has(key) ? transferStore.get(key) : null;
    window.restoreTransfer = (values, expected, signal) => new Promise(resolve => {
      const call = { values: structuredClone(values), expected: structuredClone(expected), aborted: false };
      restoreCalls.push(call);
      const abort = () => { call.aborted = true; resolve({ status: 'unavailable' }); };
      signal.addEventListener('abort', abort, { once: true });
      const finish = () => {
        if (signal.aborted) return;
        signal.removeEventListener('abort', abort);
        for (const [key, value] of Object.entries(expected)) {
          if ((transferStore.has(key) ? transferStore.get(key) : null) !== value) { resolve({ status: 'conflict' }); return; }
        }
        if (window.restoreMode === 'unavailable') { resolve({ status: 'unavailable' }); return; }
        if (window.restoreMode === 'storage-error') { resolve({ status: 'storage-error', partial: false }); return; }
        if (window.restoreMode === 'partial') { resolve({ status: 'storage-error', partial: true }); return; }
        for (const [key, value] of Object.entries(values)) transferStore.set(key, value);
        resolve({ status: 'ok', persisted: true });
      };
      if (window.restoreMode === 'delayed') window.releaseTransferRestore = finish; else finish();
    });
    window.mountTransfer = () => transferAPI.mountNativeSettingsTransfer({
      root: document.body, readItem: window.readItem, restore: window.restoreTransfer,
    });
    window.transfer = window.mountTransfer();
  }, { stored, restoreMode });
  return { context, page, requests };
}

test('filter colors use the same browser validator as the native filter editor', async t => {
  const { page, requests } = await fixture(t);
  const results = await page.evaluate(() => ['#ff0000', 'rgb(10, 20, 30)', 'red; background:url(/attack)',
    'var(--color)', 'inherit', 'not-a-color'].map(color => transferAPI.validateTransferFilters(JSON.stringify([
      { type: 2, pattern: 'needle', boards: 'demo', active: true, color },
    ])).status));
  assert.deepEqual(results, ['ok', 'ok', 'invalid', 'invalid', 'invalid', 'invalid']);
  const catalogResults = await page.evaluate(() => ['#ff0000', 'rgb(10, 20, 30)', 'red; background:url(/attack)',
    'var(--color)', 'inherit', 'not-a-color'].map(color => transferAPI.validateTransferCatalogFilters(JSON.stringify({
      0: { pattern: 'needle', boards: '', active: 1, color },
    })).status));
  assert.deepEqual(catalogResults, results);
  assert.deepEqual(requests, []);
});

test('export dialog is inert, readonly and canonical while hostile filter text remains text', async t => {
  const hostileFilter = JSON.stringify([{
    type: 2, pattern: '<img src=/settings-transfer-attack onerror=alert(1)>', boards: 'demo', active: true, auto: false,
  }]);
  const { page, requests } = await fixture(t, { stored: {
    '4chan-settings': publicDefaultSettingsRaw, '4chan-filters': hostileFilter, '4chan-css': cssRaw, 'catalog-settings': catalogRaw,
  } });
  await page.evaluate(() => transfer.openExport(document.getElementById('opener')));
  const dialog = page.getByRole('dialog', { name: 'Export Settings', exact: true });
  await assert.doesNotReject(() => dialog.waitFor({ state: 'visible' }));
  const field = dialog.getByLabel('Settings export URL', { exact: true });
  assert.equal(await field.getAttribute('readonly'), '');
  const url = await field.inputValue();
  assert.ok(url.startsWith(`${origin}/demo/#cfg=`));
  assert.equal(await dialog.locator('img, script').count(), 0);
  assert.match(await dialog.locator('.settingsTransferCompatibility').textContent(), /forceHTTPS, unmuteWebm/);
  assert.equal(await dialog.getByRole('link', { name: 'Restore Settings', exact: true }).getAttribute('href'), url);
  assert.deepEqual(requests, []);
  const decoded = parseSettingsTransferHash(new URL(url).hash);
  assert.equal(decoded.status, 'ok');
  assert.equal(decoded.payload.settings, publicDefaultSettingsRaw);
  assert.equal(decoded.payload.filters, hostileFilter);
});

test('incoming transfer requires visible review before one restore transaction', async t => {
  const incoming = JSON.stringify({ quotePreview: false, customCSS: true });
  const hash = payloadHash({ settings: incoming, filters: filterRaw, css: cssRaw, catalogFilters: catalogFiltersRaw, catalogSettings: catalogRaw });
  const { page } = await fixture(t, { hash, stored: { '4chan-settings': settingsRaw } });
  const dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.waitFor({ state: 'visible' });
  assert.equal(await page.evaluate(() => location.hash), '');
  assert.equal(await page.evaluate(() => restoreCalls.length), 0);
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), settingsRaw);
  assert.match(await dialog.textContent(), /Nothing is changed until you choose Restore Settings/);
  assert.match(await dialog.textContent(), /customCSS: true/);
  assert.match(await dialog.textContent(), /quotePreview: false/);
  assert.equal(await dialog.locator('.settingsTransferFilters pre').textContent(), filterRaw);
  assert.equal(await dialog.locator('.settingsTransferCSS pre').textContent(), cssRaw);
  assert.equal(await dialog.locator('.settingsTransferCatalogFilters pre').textContent(), catalogFiltersRaw);
  assert.match(await dialog.textContent(), /Catalog display preferences/);
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 1 && transferStore.get('4chan-settings') !== undefined);
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), incoming);
  assert.equal(await page.evaluate(() => transferStore.get('catalog-filters')), catalogFiltersRaw);
  assert.equal(await dialog.getByRole('status').textContent(), 'Settings restored.');
  assert.equal(await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).isDisabled(), true);
  assert.deepEqual(await page.evaluate(() => restoredEvents), [{ persisted: true, keys: SETTINGS_TRANSFER_STORAGE_KEYS }]);
});

test('newer storage wins over a stale review and malformed catalog filters never reach restore', async t => {
  const incoming = JSON.stringify({ quotePreview: false });
  const { page } = await fixture(t, { hash: payloadHash({ settings: incoming }), stored: { '4chan-settings': settingsRaw } });
  const dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.waitFor({ state: 'visible' });
  const newer = JSON.stringify({ quotePreview: true, backlinks: false });
  await page.evaluate(newer => transferStore.set('4chan-settings', newer), newer);
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 1);
  assert.match(await dialog.getByRole('status').textContent(), /changed after this review opened/i);
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), newer);

  await page.evaluate(hash => { location.hash = hash; }, payloadHash({ settings: incoming, catalogFilters: '[]' }));
  const error = page.locator('#settingsTransferError');
  await error.waitFor({ state: 'visible' });
  assert.match(await error.textContent(), /supported catalog rule format/i);
  assert.equal(await page.evaluate(() => restoreCalls.length), 1);
});

test('delayed restore is cancelled on close and detach', async t => {
  const incoming = JSON.stringify({ quotePreview: false });
  const { page } = await fixture(t, {
    hash: payloadHash({ settings: incoming }), stored: { '4chan-settings': settingsRaw }, restoreMode: 'delayed',
  });
  let dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 1);
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  assert.equal(await page.evaluate(() => restoreCalls[0].aborted), true);
  await page.evaluate(() => releaseTransferRestore?.());
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), settingsRaw);

  await page.evaluate(hash => { location.hash = hash; }, payloadHash({ settings: incoming }));
  dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.waitFor({ state: 'visible' });
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 2);
  await page.evaluate(() => document.body.remove());
  await page.waitForFunction(() => restoreCalls[1].aborted === true);
  await page.evaluate(() => releaseTransferRestore?.());
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), settingsRaw);
});

test('persisted pagehide cancels a delayed restore and reopens the same stale review on BFCache restore', async t => {
  const incoming = JSON.stringify({ quotePreview: false });
  const { page } = await fixture(t, {
    hash: payloadHash({ settings: incoming }), stored: { '4chan-settings': settingsRaw }, restoreMode: 'delayed',
  });
  let dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 1);
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.equal(await page.evaluate(() => restoreCalls[0].aborted), true);
  assert.equal(await page.locator('#restoreSettings').count(), 0);
  const newer = JSON.stringify({ quotePreview: true, backlinks: false });
  await page.evaluate(newer => transferStore.set('4chan-settings', newer), newer);
  await page.evaluate(() => { restoreMode = 'ok'; dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })); });
  dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await dialog.waitFor({ state: 'visible' });
  await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await page.waitForFunction(() => restoreCalls.length === 2);
  assert.match(await dialog.getByRole('status').textContent(), /changed after this review opened/i);
  assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), newer);
});

test('restore reports unavailable and storage rollback failures without claiming success', async t => {
  for (const [mode, message] of [
    ['unavailable', /persistent browser storage or cross-tab locking is unavailable/i],
    ['storage-error', /previous stored values were restored/i],
    ['partial', /rollback could not fully restore/i],
  ]) {
    await t.test(mode, async t => {
      const { page } = await fixture(t, {
        hash: payloadHash({ settings: JSON.stringify({ quotePreview: false }) }),
        stored: { '4chan-settings': settingsRaw }, restoreMode: mode,
      });
      const dialog = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
      await dialog.getByRole('button', { name: 'Restore Settings', exact: true }).click();
      await page.waitForFunction(() => restoreCalls.length === 1);
      assert.match(await dialog.getByRole('status').textContent(), message);
      assert.equal(await page.evaluate(() => transferStore.get('4chan-settings')), settingsRaw);
    });
  }
});
