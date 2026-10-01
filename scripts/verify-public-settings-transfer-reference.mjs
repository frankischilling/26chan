import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2), root = new URL('../', import.meta.url);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <pinned-reference-directory> [--write]');
const pin = JSON.parse(await readFile(new URL('docs/public-watcher-assets.json', root)));
const bytes = await readFile(resolve(args[0], 'extension.1191.js'));
assert.equal(createHash('sha256').update(bytes).digest('hex'), pin.source_sha256);
const keys = ['4chan-settings', '4chan-filters', '4chan-css', 'catalog-filters', 'catalog-settings'];
const settings = JSON.stringify({ disableAll: true, quotePreview: false });
const filters = JSON.stringify([{ type: 2, pattern: 'paper', boards: 'demo', active: true }]);
const catalogFilters = JSON.stringify({ 0: { active: 1, pattern: 'paper', boards: '', hidden: 1, top: 0 } });
const css = '.reply { color: #112233; }';
const catalogSettings = JSON.stringify({ orderby: 'r', large: true, extended: false });
const optional = { '4chan-filters': filters, '4chan-css': css, 'catalog-filters': catalogFilters, 'catalog-settings': catalogSettings };
const browser = await chromium.launch(), exports = [], restores = [];
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const page = await browser.newPage();
  const errors = [], requests = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    if (url.origin === 'https://reference.invalid' && url.pathname === '/demo/' && route.request().isNavigationRequest()) {
      return route.fulfill({ contentType: 'text/html', body: '<!doctype html><meta charset="utf-8"><title>Owned transfer shell</title>' });
    }
    requests.push(url.href); return route.abort();
  });
  await page.goto('https://reference.invalid/demo/');
  await page.evaluate(settings => {
    window.style_group = 'ws_style'; localStorage.setItem('4chan-settings', settings);
    window.publicInitCount = 0; document.addEventListener('4chanMainInit', () => publicInitCount++);
  }, settings);
  // Load the entire unchanged client after DOMContentLoaded. Its Main.init runs;
  // the board's Main.run listener is registered too late to run on this shell.
  await page.addScriptTag({ content: bytes.toString('utf8') });
  assert.deepEqual(await page.evaluate(() => ({ board: Main.board, initialized: publicInitCount, disabled: Config.disableAll })),
    { board: 'demo', initialized: 1, disabled: true });
  await page.evaluate(() => {
    const get = Storage.prototype.getItem, set = Storage.prototype.setItem;
    window.transferReads = []; window.transferWrites = [];
    Storage.prototype.getItem = function (key) { transferReads.push(key); return get.call(this, key); };
    Storage.prototype.setItem = function (key, value) { transferWrites.push({ key, value }); return set.call(this, key, value); };
  });
  for (const [name, stored] of [
    ['absent-optionals', {}], ['empty-optionals', Object.fromEntries(Object.keys(optional).map(key => [key, '']))],
    ['all-fields', optional], ['empty-catalog-rules', { 'catalog-filters': '{}' }],
    ['numeric-rule-keys', { 'catalog-filters': JSON.stringify({ 7: { active: true, pattern: 'paper', boards: 'demo', hidden: false, top: true } }) }],
  ]) {
    const result = await page.evaluate(({ keys, stored, settings }) => {
      for (const key of keys) localStorage.removeItem(key);
      localStorage.setItem('4chan-settings', settings);
      for (const [key, raw] of Object.entries(stored)) localStorage.setItem(key, raw);
      localStorage.setItem('4chan-watch', '{"owned-private-state":1}');
      transferReads = []; transferWrites = [];
      const encoded = Config.toURL();
      return { stored, payload: JSON.parse(decodeURIComponent(encoded)), reads: transferReads, writes: transferWrites };
    }, { keys, stored, settings });
    assert.deepEqual(result.reads, keys); assert.deepEqual(result.writes, []);
    exports.push({ name, ...result }); assert.deepEqual(errors, []);
  }
  for (const [name, payload] of [
    ['settings-only', { settings }],
    ['all-fields', { settings, filters, css, catalogFilters, catalogSettings }],
    ['empty-optionals', { settings, filters: '', css: '', catalogFilters: '', catalogSettings: '' }],
    ['empty-catalog-rules', { settings, catalogFilters: '{}' }],
  ]) {
    const result = await page.evaluate(({ keys, payload }) => {
      for (const key of keys) localStorage.removeItem(key);
      history.replaceState(null, '', '/demo/#cfg=' + encodeURIComponent(JSON.stringify(payload)));
      transferWrites = [];
      const restored = Config.loadFromURL();
      return { payload, restored, hash: location.hash, writes: transferWrites,
        stored: Object.fromEntries(keys.map(key => [key, localStorage.getItem(key)])) };
    }, { keys, payload });
    assert.equal(result.restored, true); assert.equal(result.hash, '');
    restores.push({ name, ...result }); assert.deepEqual(errors, []);
  }
  assert.deepEqual(requests, []); assert.deepEqual(errors, []);
  await page.close(); assert.deepEqual(errors, []);
} finally { await browser.close(); }
const result = { collection_date: '2026-10-01', source: pin.source, sha256: pin.source_sha256, bytes: bytes.length,
  browser: '151.0.7922.34',
  scope: 'Whole unchanged public v1191 loaded on an owned empty shell after DOMContentLoaded; Main.init completed, board Main.run did not run. Direct Config.toURL/loadFromURL entry points with synthetic preferences and denied nonfixture traffic. No original-page export layout, focus, board initialization or production data.',
  exports, restores };
const target = new URL('docs/public-settings-transfer-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${exports.length} public transfer exports and ${restores.length} restores.`);
