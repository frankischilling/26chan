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
const icons = JSON.parse(await readFile(new URL('docs/public-catalog-filter-assets.json', root)));
for (const asset of icons.assets) {
  const bytes = await readFile(resolve(args[0], 'catalog-filter-' + asset.name.replace('/', '-')));
  assert.equal(bytes.length, asset.bytes); assert.equal(digest(bytes), asset.sha256);
  assert.ok(bytes.equals(await readFile(new URL('apps/public' + icons.local_base + asset.name, root))));
}
const catalog = { slug: 'demo', anon: 'Anonymous', count: 3, flags: false, threads: {
  1000001: { author: "Avery", trip: "!Origami", b: 2, sub: 'Owned crane', teaser: 'One sheet', file: 'crane.png', s: 1, r: 2, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
  1000002: { author: "Riley", trip: "!Paper", capcode: "mod", b: 1, sub: 'Owned boat', teaser: 'Two folds', file: 'boat.png', s: 2, r: 1, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
  1000003: { author: "Anonymous", sub: "Feeling fold", teaser: "paper feeling", b: 0, s: 3, r: 0, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
}, order: { alt: [1000003, 1000002, 1000001], absdate: [1000001, 1000002, 1000003], date: [1000003, 1000002, 1000001], r: [1000001, 1000002, 1000003] } };
const auxiliary = ['settingsWindowLink', 'settingsWindowLinkBot', 'settingsWindowLinkMobile', 'togglePostFormLinkMobile',
  'filtered-label', 'hidden-label', 'filtered-label-bottom', 'hidden-label-bottom', 'filtered-count', 'filtered-count-bottom',
  'hidden-count', 'hidden-count-bottom', 'ordered-by', 'last-updated', 'last-updated-bottom', 'filters-clear-hidden', 'filters-clear-hidden-bottom'];
function html(css) {
  return `<!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css}\n${mobile.text}</style><link id="mobile-css">
<div id="backdrop" class="hidden"></div><div id="boardNavDesktop"><span class="boardList"></span></div><div id="boardNavDesktopFoot"><span class="boardList"></span></div><div id="boardNavMobile"><select id="boardSelectMobile"><option value="/demo/catalog">demo</option></select><span></span></div>
<form name="post"><table id="postForm"></table></form><div id="ctrl"><div id="info">
<span id="search-label">Search: <span id="search-term"></span></span></div><hr class="mobile">
<div id="settings" class="mobilebtn"><span class="ctrl-wrap">Sort by: <select id="order-ctrl"><option value="alt">Bump order</option><option value="absdate">Last reply</option><option value="date">Creation date</option><option value="r">Reply count</option></select></span>
<span class="ctrl-wrap">Image size: <select id="size-ctrl"><option value="small">Small</option><option value="large">Large</option></select></span>
<span class="ctrl-wrap">Teasers: <select id="teaser-ctrl"><option value="off">Off</option><option value="on">On</option></select></span>
<span class="btn-wrap"><span id="filters-ctrl" class="button">Filters</span></span>
<span class="btn-wrap"><span id="qf-ctrl" class="button">Search</span></span><span id="qf-cnt"><input id="qf-box" name="qf-box" type="text"><span id="qf-clear" class="button">×</span></span>
</div><div class="clear"></div></div><hr><div id="threads"></div><span id="search-label-bottom">Search: <span id="search-term-bottom"></span></span>
${auxiliary.map(id => id.startsWith('settingsWindowLink') ? `<a id="${id}" href="#settings">Settings</a>` : `<span id="${id}"></span>`).join('')}
<select id="styleSelector"><option value="Yotsuba B New">Yotsuba B</option></select><input id="owned-input"><textarea id="owned-textarea"></textarea><button id="owned-button">Owned button</button><a id="owned-link" href="#bottom">Owned link</a><div id="bottom"></div>`;
}
const browser = await chromium.launch();
const states = [], nativeDefaults = [], shortcuts = [], reloads = [], cases = [];
const pageErrors = new WeakMap();
async function closePage(page) {
  assert.deepEqual(pageErrors.get(page), [], 'Pinned catalog client raised a page error');
  await page.close();
}
async function pageFor(style, width, hash = '', stored = {}, density = 1) {
  const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: density });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.stack));
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    return url.origin === 'https://reference.invalid' && url.pathname === '/' && !url.search
      && route.request().isNavigationRequest()
      ? route.fulfill({ body: html(style.text), contentType: 'text/html' }) : route.abort();
  });
  await page.goto('https://reference.invalid/' + hash);
  await page.clock.install({ time: new Date('2026-09-08T12:00:00Z') });
  await page.evaluate(({ name, stored }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    document.cookie = `ws_style=${encodeURIComponent(name.replaceAll('_', ' ').replace(/\b[a-z]/g, value => value.toUpperCase()))}; Path=/`;
    for (const [key, value] of Object.entries(stored)) {
      const storage = key.startsWith('4chan-catalog-search') ? sessionStorage : localStorage;
      if (value === null) storage.removeItem(key); else storage.setItem(key, typeof value === 'string' ? value : JSON.stringify(value));
    }
  }, { name: style.name, stored });
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

async function record(page, label, destination = states) {
  const value = await page.evaluate(() => {
    const panel = document.getElementById('theme');
    const shown = panel && !panel.classList.contains('hidden');
    return {
      shown: !!shown,
      fields: panel ? ['nobinds', 'nospoiler', 'newtab', 'tw', 'ddn'].map(key => {
        const input = document.getElementById('theme-' + key);
        return { key, checked: input.checked, rowDisplay: getComputedStyle(input.closest('li')).display };
      }) : [],
      css: panel ? document.getElementById('theme-css').value : null,
      focus: shown ? document.activeElement.id : null,
      spoilerClass: document.body.classList.contains('reveal-img-spoilers'),
      links: [...document.querySelectorAll('#threads > .thread > a')].map(link => ({ id: link.parentElement.id, target: link.getAttribute('target') })),
      stored: { theme: localStorage.getItem('catalog-theme'), settings: localStorage.getItem('4chan-settings') },
    };
  });
  destination.push({ label, width: page.viewportSize().width, ...value });
  assert.deepEqual(pageErrors.get(page), [], label);
}
async function shortcutState(page, label, event) {
  const value = await page.evaluate(() => ({
    shown: getComputedStyle(document.getElementById('qf-cnt')).display,
    active: document.getElementById('qf-ctrl').classList.contains('active'),
    input: document.getElementById('qf-box').value,
    order: document.getElementById('order-ctrl').value,
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => node.id),
  }));
  shortcuts.push({ label, width: page.viewportSize().width, event, ...value });
  assert.deepEqual(pageErrors.get(page), [], label);
}
async function dispatch(page, type, target, code, modifiers = {}) {
  return page.evaluate(({ type, target, code, modifiers }) => {
    const element = target === 'body' ? document.body : document.getElementById(target);
    const event = new KeyboardEvent(type, { key: String.fromCharCode(code).toLowerCase(), keyCode: code, bubbles: true, cancelable: true, ...modifiers });
    element.dispatchEvent(event);
    return { type, target, code, modifiers, prevented: event.defaultPrevented };
  }, { type, target, code, modifiers });
}
async function panelStyle(page) {
  return page.evaluate(() => Object.fromEntries([
    ['panel', '#theme', ['width', 'fontFamily', 'fontSize', 'color', 'backgroundColor', 'padding', 'borderWidth', 'borderColor', 'borderRadius', 'boxShadow']],
    ['header', '#theme .panelHeader', ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'margin', 'padding', 'borderBottomWidth', 'borderBottomColor', 'textAlign']],
    ['heading', '#theme h4', ['fontSize', 'fontWeight', 'margin', 'padding']],
    ['list', '#theme ul.clickset', ['margin', 'padding', 'listStyleType']],
    ['row', '#theme ul.clickset li', ['margin', 'padding', 'lineHeight']],
    ['checkbox', '#theme-nospoiler', ['margin', 'padding']],
    ['css', '#theme-css', ['width', 'height', 'fontFamily', 'fontSize', 'margin', 'padding', 'borderWidth', 'borderColor', 'color', 'backgroundColor', 'boxSizing']],
    ['actions', '#theme-btns', ['margin', 'padding', 'textAlign']],
    ['submit', '#theme-save', ['fontFamily', 'fontSize', 'padding', 'margin']],
  ].map(([name, selector, properties]) => {
    const style = getComputedStyle(document.querySelector(selector));
    return [name, Object.fromEntries(properties.map(property => [property, style[property]]))];
  })));
}
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const style = styles.find(row => row.theme === 'yotsuba-b');
  for (const width of [1280, 390]) {
    const page = await pageFor(style, width);
    await record(page, 'initial');
    await page.locator('#settingsWindowLink').click({ force: true }); await record(page, 'opened-defaults');
    await page.locator('#theme-nospoiler').check(); await page.locator('#theme-newtab').check();
    // Desktop-only controls still belong to the saved public format on mobile.
    await page.locator('#theme-nobinds').evaluate(node => node.checked = true);
    await record(page, 'checked-unsaved');
    await page.locator('#theme-save').click(); await record(page, 'saved-three-options');
    await page.locator('#settingsWindowLink').click({ force: true }); await record(page, 'reopened-three-options');
    for (const key of ['nobinds', 'nospoiler', 'newtab']) await page.locator('#theme-' + key).evaluate(node => node.checked = false);
    await page.locator('#theme-save').click(); await record(page, 'saved-empty-theme');
    await page.locator('#settingsWindowLink').click({ force: true });
    await page.locator('#theme-css').fill('.teaser { color: #008000; }'); await record(page, 'css-unsaved');
    await page.locator('#theme-close').click(); await record(page, 'closed-unsaved');
    await page.locator('#settingsWindowLink').click({ force: true }); await record(page, 'reopened-unsaved-css');
    await page.locator('#theme-save').click(); await record(page, 'saved-css');
    await page.locator('#settingsWindowLink').click({ force: true }); await record(page, 'reopened-css');
    await page.locator('#theme-css').fill(''); await page.locator('#theme-save').click(); await record(page, 'cleared-css');
    await closePage(page);
    for (const [label, target, modifiers] of [
      ['body', 'body', {}], ['button', 'owned-button', {}], ['link', 'owned-link', {}], ['select', 'order-ctrl', {}],
      ['input', 'owned-input', {}], ['textarea', 'owned-textarea', {}], ['control', 'body', { ctrlKey: true }],
      ['alt', 'body', { altKey: true }], ['shift', 'body', { shiftKey: true }], ['meta', 'body', { metaKey: true }],
    ]) {
      const page = await pageFor(style, width);
      await shortcutState(page, label + '-keydown', await dispatch(page, 'keydown', target, 83, modifiers));
      await shortcutState(page, label + '-keyup', await dispatch(page, 'keyup', target, 83, modifiers));
      await closePage(page);
    }
    const disabled = await pageFor(style, width, '', { 'catalog-theme': { nobinds: true } });
    await shortcutState(disabled, 'disabled-keyup', await dispatch(disabled, 'keyup', 'body', 83)); await closePage(disabled);

    for (const [label, target, modifiers, disabled] of [
      ['body', 'body', {}, false], ['button', 'owned-button', {}, false], ['select', 'order-ctrl', {}, false],
      ['input', 'owned-input', {}, false], ['textarea', 'owned-textarea', {}, false],
      ['shift', 'body', { shiftKey: true }, false], ['control', 'body', { ctrlKey: true }, false],
      ['disabled', 'body', {}, true],
    ]) {
      const page = await pageFor(style, width, '', disabled ? { 'catalog-theme': { nobinds: true } } : {});
      let navigations = 0;
      const original = page.url();
      page.on('framenavigated', frame => { if (frame === page.mainFrame()) navigations++; });
      const event = await dispatch(page, 'keyup', target, 82, modifiers);
      // A same-document refresh crosses the browser/HTTP boundary, independently
      // of the paused catalog clock. The owned document has no remote content.
      await new Promise(resolve => setTimeout(resolve, 150));
      await page.waitForLoadState('load');
      reloads.push({ label, width, event, navigations, samePage: page.url() === original });
      await closePage(page);
    }
    const cycle = await pageFor(style, width);
    for (let index = 0; index < 4; index++) await shortcutState(cycle, 'cycle-' + index, await dispatch(cycle, 'keyup', 'body', 88));
    await closePage(cycle);
  }
  for (const style of styles) for (const width of [1280, 390]) {
    const page = await pageFor(style, width);
    await page.locator('#settingsWindowLink').click({ force: true });
    cases.push({ theme: style.theme, width, values: await panelStyle(page) });
    await closePage(page);
  }
  for (const width of [1280, 390]) for (const [label, settings] of [
    ['absent', null], ['empty', {}], ['explicit-off', { threadWatcher: false, dropDownNav: false }],
    ['explicit-on', { threadWatcher: true, dropDownNav: true }],
  ]) {
    const page = await pageFor(style, width, '', { '4chan-settings': settings });
    await page.locator('#settingsWindowLink').click({ force: true });
    await record(page, label + '-opened', nativeDefaults);
    await page.locator('#theme-save').click(); await record(page, label + '-saved', nativeDefaults);
    await closePage(page);
  }
} finally { await browser.close(); }
const observed = { scope: 'Whole unchanged pinned catalog client on three owned cards. Settings fields, persistence, CSS editor state and keyboard phase/target/modifier cases. All nonfixture requests denied; no production content.',
  client: { url: 'https://s.4cdn.org/' + client.pin.path, bytes: client.pin.bytes, sha256: client.pin.sha256 },
  environment: { chromium: browser.version(), viewports: [[1280, 900], [390, 900]], density: 1, clock: '2026-09-08T12:00:00Z', paused_at: '2026-09-08T13:00:00Z' },
  styles: [...styles.map(row => row.pin), mobile.pin], catalog, states, nativeDefaults, shortcuts, reloads, cases };
const destination = new URL('docs/public-catalog-settings-reference.json', root);
if (args[1] === '--write') await writeFile(destination, JSON.stringify(observed, null, 2) + '\n');
else assert.deepEqual(observed, JSON.parse(await readFile(destination)));
console.log(`PASS ${states.length} catalog settings states, ${nativeDefaults.length} native defaults, ${shortcuts.length} shortcut states, ${reloads.length} reload cases and ${cases.length} style cases`);
