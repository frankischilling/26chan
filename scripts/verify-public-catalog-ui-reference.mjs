// Whole pinned catalog client on owned data and static selector shape only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <pinned-reference-directory> [--write]');
const root = new URL('../', import.meta.url);
const manifest = JSON.parse(await readFile(new URL('docs/public-catalog-reference.json', root)));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
async function pinned(name) {
  const pin = manifest.assets.find(asset => asset.path.endsWith('/' + name));
  assert.ok(pin, name);
  const bytes = await readFile(resolve(args[0], name));
  assert.equal(bytes.length, pin.bytes); assert.equal(digest(bytes), pin.sha256);
  return { pin, text: bytes.toString('utf8') };
}
const client = await pinned('catalog.min.1025.js');
const mobile = await pinned('catalog_mobile.705.css');
const themes = Object.entries(manifest.themes);
const styles = await Promise.all(themes.map(async ([theme, name]) => ({ theme, name, ...(await pinned(`catalog_${name}.705.css`)) })));
const controlsAssets = JSON.parse(await readFile(new URL('docs/public-catalog-control-assets.json', root)));
for (const asset of controlsAssets) {
  const bytes = await readFile(resolve(args[0], new URL(asset.url).pathname.split('/').pop()));
  assert.equal(bytes.length, asset.bytes); assert.equal(digest(bytes), asset.sha256);
  assert.ok(bytes.equals(await readFile(new URL(asset.path, root))));
}
const catalog = { slug: 'demo', anon: 'Anonymous', count: 2, flags: false, threads: {
  1000001: { sub: 'Owned crane', teaser: 'One sheet', file: 'crane.png', s: 1, r: 2, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
  1000002: { sub: 'Owned boat', teaser: 'Two folds', file: 'boat.png', s: 2, r: 1, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
}, order: { alt: [1000001, 1000002], absdate: [1000001, 1000002], date: [1000002, 1000001], r: [1000001, 1000002] } };
const auxiliary = ['settingsWindowLink', 'settingsWindowLinkBot', 'settingsWindowLinkMobile', 'togglePostFormLinkMobile',
  'filtered-label', 'hidden-label', 'filtered-label-bottom', 'hidden-label-bottom', 'filtered-count', 'filtered-count-bottom',
  'hidden-count', 'hidden-count-bottom', 'ordered-by', 'last-updated', 'last-updated-bottom', 'filters-clear-hidden', 'filters-clear-hidden-bottom'];
function html(css) {
  return `<!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css}\n${mobile.text}</style><link id="mobile-css">
<div id="boardNavDesktop"></div><div id="boardNavDesktopFoot"></div><div id="boardNavMobile"></div>
<form name="post"><table id="postForm"></table></form><div id="ctrl"><div id="info">
<span id="search-label">Search: <span id="search-term"></span></span></div><hr class="mobile">
<div id="settings" class="mobilebtn"><span class="ctrl-wrap">Sort by: <select id="order-ctrl"><option value="alt">Bump order</option></select></span>
<span class="ctrl-wrap">Image size: <select id="size-ctrl"><option value="small">Small</option></select></span>
<span class="ctrl-wrap">Teasers: <select id="teaser-ctrl"><option value="on">On</option></select></span>
<span class="btn-wrap"><span id="filters-ctrl" class="button">Filters</span></span>
<span class="btn-wrap"><span id="qf-ctrl" class="button">Search</span></span><span id="qf-cnt"><input id="qf-box" name="qf-box" type="text"><span id="qf-clear" class="button">×</span></span>
</div><div class="clear"></div></div><hr><div id="threads"></div><span id="search-label-bottom">Search: <span id="search-term-bottom"></span></span>
${auxiliary.map(id => `<span id="${id}"></span>`).join('')}
<select id="styleSelector"><option value="Yotsuba B New">Yotsuba B</option></select><div id="bottom"></div>`;
}
const browser = await chromium.launch();
const behavior = [], cases = [];
const pageErrors = new WeakMap();
async function closePage(page) {
  assert.deepEqual(pageErrors.get(page), [], 'Pinned catalog client raised a page error');
  await page.close();
}
async function pageFor(style, width, hash = '', saved = null) {
  const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: 1 });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    return url.origin === 'https://reference.invalid' && url.pathname === '/' && !url.search
      && route.request().isNavigationRequest()
      ? route.fulfill({ body: html(style.text), contentType: 'text/html' }) : route.abort();
  });
  await page.goto('https://reference.invalid/' + hash);
  await page.clock.install({ time: new Date('2026-09-08T12:00:00Z') });
  await page.evaluate(({ name, saved }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    document.cookie = `ws_style=${encodeURIComponent(name.replaceAll('_', ' ').replace(/\b[a-z]/g, value => value.toUpperCase()))}; Path=/`;
    if (saved) { sessionStorage.setItem('4chan-catalog-search', saved.query); sessionStorage.setItem('4chan-catalog-search-board', saved.board); }
  }, { name: style.name, saved });
  await page.addScriptTag({ content: client.text });
  await page.evaluate(catalog => {
    window.$L = { d: () => 'reference.invalid' };
    window.fourcat = new FC();
    fourcat.applyCSS(null, 'ws_style', 705);
    fourcat.init(); fourcat.loadCatalog(catalog);
  }, catalog);
  await page.clock.pauseAt(new Date('2026-09-08T13:00:00Z'));
  assert.deepEqual(errors, []);
  return page;
}
async function state(page, label) {
  const value = await page.evaluate(() => ({
    display: getComputedStyle(document.getElementById('qf-cnt')).display,
    active: document.getElementById('qf-ctrl').classList.contains('active'),
    value: document.getElementById('qf-box').value,
    focus: getComputedStyle(document.getElementById('qf-cnt')).display === 'none' ? null : document.activeElement.id,
    labels: ['search-label', 'search-label-bottom'].map(id => getComputedStyle(document.getElementById(id)).display),
    terms: ['search-term', 'search-term-bottom'].map(id => document.getElementById(id).textContent),
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => node.id),
    stored: { query: sessionStorage.getItem('4chan-catalog-search'), board: sessionStorage.getItem('4chan-catalog-search-board') },
  }));
  behavior.push({ label, width: page.viewportSize().width, ...value });
  assert.deepEqual(pageErrors.get(page), [], `Pinned catalog client raised an error during ${label}`);
}
async function styleValues(page) {
  return page.evaluate(() => Object.fromEntries([
    ['settings', '#settings', ['float', 'textAlign', 'lineHeight']],
    ['wrapper', '#qf-ctrl', ['fontFamily', 'fontSize', 'fontWeight', 'color', 'borderWidth', 'borderColor', 'borderStyle', 'borderRadius', 'padding', 'whiteSpace', 'backgroundColor', 'backgroundImage', 'backgroundRepeat']],
    ['button', '#qf-ctrl', ['cursor', 'userSelect', 'whiteSpace']],
    ['brackets', '#qf-ctrl', []],
    ['container', '#qf-cnt', ['display']],
    ['input', '#qf-box', ['width', 'height', 'fontSize', 'fontFamily', 'padding', 'margin', 'boxSizing', 'borderWidth', 'borderColor', 'borderStyle', 'lineHeight', 'color', 'backgroundColor']],
    ['clear', '#qf-clear', ['textDecorationLine', 'borderWidth', 'fontSize']],
  ].map(([name, selector, properties]) => {
    const node = name === 'wrapper' || name === 'brackets' ? document.querySelector(selector).parentElement : document.querySelector(selector);
    const style = getComputedStyle(node);
    return [name, name === 'brackets' ? { before: getComputedStyle(node, '::before').content, after: getComputedStyle(node, '::after').content }
      : Object.fromEntries(properties.map(property => [property, style[property]]))];
  })));
}
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const style = styles.find(row => row.theme === 'yotsuba-b');
  for (const width of [1280, 390]) {
    const page = await pageFor(style, width);
    await state(page, 'initial');
    await page.locator('#qf-ctrl').click(); await state(page, 'opened');
    await page.locator('#qf-box').fill('crane'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(249); await state(page, 'debounce-249');
    await page.clock.runFor(1); await state(page, 'debounce-250');
    await page.locator('#qf-box').press('Escape'); await state(page, 'escape');
    await page.locator('#qf-ctrl').click(); await state(page, 'reopened');
    await page.locator('#qf-box').fill('boat'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(250); await page.locator('#qf-clear').click(); await state(page, 'close-button');
    await page.keyboard.press('s'); await state(page, 'shortcut-open');
    await page.locator('#qf-box').fill('crane'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(250); await page.locator('#qf-box').evaluate(node => node.blur());
    await page.keyboard.press('s'); await state(page, 'shortcut-clears-active-input');
    await page.locator('#qf-box').press('ArrowRight'); await page.clock.runFor(250); await state(page, 'cleared-input-applied');
    await page.locator('#qf-box').fill('boat'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(100); await page.locator('#qf-ctrl').click(); await state(page, 'pending-close');
    await page.clock.runFor(150); await state(page, 'pending-close-250');
    await closePage(page);
    for (const [label, hash, saved] of [['same-board-session', '', { query: 'crane', board: 'demo' }],
      ['different-board-session', '', { query: 'crane', board: 'other' }], ['fragment-search', '#s=Owned+boat', null]]) {
      const page = await pageFor(style, width, hash, saved); await state(page, label); await closePage(page);
    }
    const inputPage = await pageFor(style, width);
    await inputPage.locator('#qf-ctrl').click();
    await inputPage.locator('#qf-box').fill('crane');
    await inputPage.clock.runFor(250); await state(inputPage, 'input-without-keyup');
    await inputPage.locator('#qf-box').dispatchEvent('compositionstart');
    await inputPage.locator('#qf-box').fill('boat'); await inputPage.locator('#qf-box').press('ArrowRight');
    await inputPage.clock.runFor(250); await state(inputPage, 'composition-keyup');
    await inputPage.locator('#qf-box').dispatchEvent('compositionend');
    await inputPage.clock.runFor(250); await state(inputPage, 'composition-end-without-keyup');
    await inputPage.locator('#qf-box').fill('crane'); await inputPage.locator('#qf-box').press('Enter');
    await state(inputPage, 'enter-immediate');
    await inputPage.clock.runFor(250); await state(inputPage, 'enter-250');
    await closePage(inputPage);
    for (const [label, modifiers] of [['control-shortcut', { ctrlKey: true }], ['alt-shortcut', { altKey: true }], ['shift-shortcut', { shiftKey: true }]]) {
      const page = await pageFor(style, width);
      await page.evaluate(modifiers => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 's', keyCode: 83, bubbles: true, cancelable: true, ...modifiers })), modifiers);
      await state(page, label); await closePage(page);
    }
  }
  for (const style of styles) for (const width of [1280, 390]) {
    const page = await pageFor(style, width);
    await page.mouse.move(0, 0);
    cases.push({ theme: style.theme, width, state: 'closed', values: await styleValues(page) });
    await page.locator('#qf-ctrl').click(); await page.mouse.move(0, 0);
    cases.push({ theme: style.theme, width, state: 'open', values: await styleValues(page) });
    await closePage(page);
  }
} finally { await browser.close(); }
const observed = { scope: 'Full pinned public catalog client executes on two owned synthetic cards and static selector shape. All external browser requests denied. This is a control reference, not an original full-page snapshot.',
  client: { url: 'https://s.4cdn.org/' + client.pin.path, bytes: client.pin.bytes, sha256: client.pin.sha256 },
  dom_observation: { url: 'https://boards.4chan.org/g/catalog', collected_utc: '2026-09-30T23:09:46.177Z', bytes: 120825,
    sha256: '678b99af5cd8a099f06c0d62906267dd816355916bffa5b701a3cecf218feade', retained: 'Static tags/classes/IDs only. No post text, values, filenames, links, embedded catalog data or response retained.' },
  environment: { chromium: '151.0.7922.34', density: 1, viewports: [[1280, 900], [390, 900]], clock: '2026-09-08T12:00:00Z', paused_at: '2026-09-08T13:00:00Z', network: 'Every nonfixture request aborted.', focus: 'Recorded only while the Search field is shown; hidden-input blur timing belongs to the browser.' },
  styles: [...styles.map(row => row.pin), mobile.pin], controlsAssets, behavior, cases };
const destination = new URL('docs/public-catalog-ui-reference.json', root);
if (args[1] === '--write') await writeFile(destination, JSON.stringify(observed, null, 2) + '\n');
else assert.deepEqual(observed, JSON.parse(await readFile(destination)));
console.log(`PASS ${behavior.length} full-client catalog control states and ${cases.length} independent style cases`);
