import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { catalogMainBootstrap } from '../../apps/public/static/catalog-theme.v1.js';

const oracle = JSON.parse(await readFile(new URL('../fixtures/native-main-init-source.json', import.meta.url)));
const snippets = Object.fromEntries(Object.entries(oracle.snippets).map(([key, value]) => [key, value.text]));
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const tick = async () => { for (let index = 0; index < 16; index++) await Promise.resolve(); };
function fixture() {
  const document = new EventTarget(), window = new EventTarget(), trace = [];
  let observer;
  window.MutationObserver = class { constructor(callback) { observer = callback; } observe() {} disconnect() {} };
  document.defaultView = window; document.body = {};
  const root = { ownerDocument: document, isConnected: true, dataset: { catalog: 'true' } };
  let canonical = root;
  document.getElementById = () => canonical;
  document.createEvent = kind => {
    assert.equal(kind, 'Event'); const event = new Event('4chanMainInit');
    event.initEvent = (name, bubbles, cancelable) => { assert.equal(name, event.type); assert.equal(bubbles, false); assert.equal(cancelable, false); };
    return event;
  };
  document.addEventListener('4chanMainInit', event => {
    assert.equal(event.constructor, Event); assert.equal(event.target, document);
    assert.equal(event.bubbles, false); assert.equal(event.cancelable, false);
    assert.equal(Object.hasOwn(event, 'detail'), false); trace.push('main');
  });
  const bootstrap = catalogMainBootstrap(root);
  const preferences = { current: () => true, prepare() { trace.push('controls'); }, load() { trace.push('load'); } };
  const transition = (name, persisted = true) => { const event = new Event(name); event.persisted = persisted; window.dispatchEvent(event); };
  return { document, root, trace, bootstrap, preferences, transition, replace() { canonical = {}; observer(); } };
}

for (const stored of [null, { orderby: 'r', large: true, extended: false }]) {
  for (const disableAll of [false, true]) test(`pinned catalog init prepares display before load (saved=${!!stored}, disabled=${disableAll})`, () => {
    const h = fixture(), controls = Object.fromEntries(['threads', 'qf-ctrl', 'teaser-ctrl', 'size-ctrl', 'order-ctrl'].map(key => [key, {}]));
    const writes = [], order = [];
    const sandbox = { self: {}, window: {}, document: h.document, options: null, catalog: {}, FC: {}, UA: { hasWebStorage: true },
      activeTheme: {}, activeStyleSheet: '', basicSettings: ['orderby', 'large', 'extended'],
      $: { id: id => controls[id] ?? {}, on() {}, extend: Object.assign, readCookie: () => null },
      localStorage: { getItem: key => key === 'catalog-settings' ? stored && JSON.stringify(stored) : key === '4chan-settings' ? JSON.stringify({ disableAll }) : null,
        setItem: (...args) => writes.push(args) },
      checkMobileLayout: () => false, applyTheme: () => order.push('theme'), bindGlobalShortcuts() {}, initGlobalMessage() {},
      CustomMenu: { initCtrl() {}, apply() {} }, ThreadWatcher: {}, showDropDownNav() {},
      buildThreads: () => order.push('build'),
    };
    for (const key of ['toggleQuickfilter', 'toggleHiddenThreads', 'showThemeEditor', 'showFilters', 'onTeaserChange', 'onSizeChange', 'onOrderChange', 'onThreadMouseOver', 'onThreadMouseOut', 'togglePostFormMobile', 'onClick']) sandbox[key] = () => {};
    h.document.forms = { post: {} };
    h.document.addEventListener('4chanMainInit', () => {
      const value = stored ?? { orderby: 'alt', large: false, extended: true };
      assert.equal(controls['order-ctrl'].selectedIndex, value.orderby === 'r' ? 3 : 0);
      assert.equal(controls['size-ctrl'].selectedIndex, Number(value.large));
      assert.equal(controls['teaser-ctrl'].selectedIndex, Number(value.extended));
      assert.equal(controls.threads.className, `${value.extended ? 'extended-' : ''}${value.large ? 'large' : 'small'}`);
      assert.deepEqual(writes, []); assert.deepEqual(order, ['theme']); order.push('main');
    });
    vm.createContext(sandbox);
    vm.runInContext('var ' + snippets.catalog_defaults + ' unused = null;\n' + snippets.catalog_dispatch + snippets.catalog_settings + snippets.catalog_init, sandbox);
    sandbox.fourcat = { init: sandbox.self.init, loadCatalog: () => order.push('load') };
    sandbox.initAnalytics = () => {};
    vm.runInContext(snippets.catalog_caller, sandbox);
    h.document.dispatchEvent(new Event('DOMContentLoaded'));
    assert.deepEqual(order, ['theme', 'main', 'load']);
    assert.ok(snippets.catalog_caller.indexOf('fourcat.init()') < snippets.catalog_caller.indexOf('fourcat.loadCatalog(catalog)'));
    assert.ok(snippets.catalog_load.indexOf('loadFilters()') < snippets.catalog_load.indexOf('buildThreads()'));
  });
}

test('the private owners rendezvous once; settings and controls precede notification and load', async () => {
  const h = fixture(), ready = deferred();
  h.bootstrap.preferences(h.preferences); h.bootstrap.settings(() => ready.promise);
  await tick(); assert.deepEqual(h.trace, []);
  ready.resolve(() => h.trace.push('settings')); await tick();
  assert.deepEqual(h.trace, ['settings', 'controls', 'main', 'load']);
  assert.equal(catalogMainBootstrap(h.root), h.bootstrap);
  h.bootstrap.preferences(h.preferences); h.bootstrap.settings(() => { throw new Error('duplicate'); });
  for (const name of ['4chanSettingsSaved', '4chanPreferencesRestored', '4chanCatalogThemeApplied']) h.document.dispatchEvent(new Event(name));
  h.transition('pagehide'); h.transition('pageshow'); await tick();
  assert.deepEqual(h.trace, ['settings', 'controls', 'main', 'load']);
});
for (const point of ['before-settings', 'theme-listener', 'main-listener']) test(`catalog suspension at ${point} resumes without replay`, async () => {
  const h = fixture(), ready = deferred();
  h.bootstrap.preferences(h.preferences);
  if (point === 'main-listener') h.document.addEventListener('4chanMainInit', () => h.transition('pagehide'));
  h.bootstrap.settings(() => ready.promise);
  if (point === 'before-settings') h.transition('pagehide');
  ready.resolve(() => { h.trace.push('settings'); if (point === 'theme-listener') h.transition('pagehide'); });
  await tick(); assert.equal(h.trace.includes('load'), false);
  h.transition('pageshow'); h.transition('pagehide'); await tick(); assert.equal(h.trace.includes('load'), false);
  h.transition('pageshow'); await tick(); assert.deepEqual(h.trace, ['settings', 'controls', 'main', 'load']);
});
for (const when of ['pending', 'main']) for (const how of ['departure', 'replacement']) test(`${how} at ${when} never loads the abandoned catalog`, async () => {
  const h = fixture(), ready = deferred();
  const leave = () => how === 'departure' ? h.transition('pagehide', false) : h.replace();
  if (when === 'main') h.document.addEventListener('4chanMainInit', leave);
  h.bootstrap.preferences(h.preferences); h.bootstrap.settings(() => ready.promise);
  if (when === 'pending') leave(); ready.resolve(() => h.trace.push('settings'));
  await tick(); h.transition('pageshow'); await tick();
  assert.equal(h.trace.includes('load'), false); assert.equal(h.trace.includes('main'), when === 'main');
});
test('failed optional settings loader falls back, failed preparation cannot announce readiness', async () => {
  const h = fixture(); h.bootstrap.preferences(h.preferences); h.bootstrap.settings(() => Promise.reject(new Error('unavailable')));
  await tick(); assert.deepEqual(h.trace, ['controls', 'main', 'load']);
  const failed = fixture(); failed.preferences.prepare = () => false;
  failed.bootstrap.preferences(failed.preferences); failed.bootstrap.settings(() => null);
  await tick(); assert.deepEqual(failed.trace, []);
});
test('forged public notifications cannot release a pending catalog owner', async () => {
  const h = fixture(); h.bootstrap.preferences(h.preferences);
  for (const name of ['4chanMainInit', '4chanParsingDone', '4chanCatalogThemeApplied', '4chanSettingsSaved']) h.document.dispatchEvent(new Event(name));
  h.trace.length = 0; h.transition('pageshow'); await tick(); assert.deepEqual(h.trace, []);
});

const preferenceSource = await readFile(new URL('../../apps/public/static/catalog-preferences.v1.js', import.meta.url), 'utf8');
const displayHandlers = preferenceSource.slice(preferenceSource.indexOf('  for (const control of [order, size, teaser])'), preferenceSource.indexOf('  if (searchReady) {\n    const openSearch'));
const initializers = preferenceSource.slice(preferenceSource.indexOf('  let url, display, spoilerChanged, originalDisplay;'), preferenceSource.indexOf('  const bootstrap = catalogMainBootstrap('));
for (const duringMain of [false, true]) test(`actual catalog display handlers defer choices until per-thread controls exist (during MainInit=${duringMain})`, () => {
  const trace = [], order = new EventTarget(), size = new EventTarget(), teaser = new EventTarget();
  order.value = 'alt'; size.value = 'small'; teaser.value = 'on';
  let installed = false;
  const current = () => ({ orderby: order.value, large: size.value === 'large', extended: teaser.value === 'on' });
  const applyDisplay = value => { order.value = value.orderby; size.value = value.large ? 'large' : 'small'; teaser.value = value.extended ? 'on' : 'off'; };
  const sandbox = { URL, location: { href: 'https://example.test/fixture/catalog', replace() { throw new Error('unexpected fallback'); } },
    order, size, teaser, current, applyDisplay, initialSpoilers: false, revealSpoilers: () => false,
    spoilerControl: false, entries: [], key: 'catalog-settings', storedPreference: JSON.parse,
    localStorage: { getItem: () => '{"orderby":"date","large":false,"extended":true}' },
    stateReady: true, pinKey: 'pins', hideKey: 'hide', pins: new Map(), hiddenThreads: new Map(),
    readState: () => new Map(), installThreadControls: () => { installed = true; trace.push('thread-controls'); },
    searchReady: true, searchControlsReady: false, search: { value: '' }, validQuery: () => true,
    renderedQuery: '', renderedOrder: 'alt', searchKey: 'search', boardKey: 'board', board: 'fixture',
    sessionStorage: { getItem: key => key === 'board' ? 'fixture' : 'stored search' },
    apply: (value, query) => { assert.equal(installed, true, 'rendering must wait for per-entry pin nodes'); trace.push(['apply', { ...value }, query]); applyDisplay(value); return true; },
    save: () => trace.push(['save', current()]), updateURL() {}, saveSearch() {},
    form: { requestSubmit() { throw new Error('unexpected form fallback'); } },
  };
  vm.createContext(sandbox);
  vm.runInContext('let catalogLoaded = false, pendingDisplay = null, pendingSearch = "typed search", pendingSpoilers = null;\n' + displayHandlers + initializers + '\nthis.prepare = prepareCatalog; this.load = loadCatalog;', sandbox);
  const choose = values => { for (const [control, value] of values) { control.value = value; control.dispatchEvent(new Event('change')); } };
  choose([[order, 'r'], [size, 'large'], [teaser, 'off']]); assert.deepEqual(trace, []);
  sandbox.prepare(); assert.deepEqual(current(), { orderby: 'r', large: true, extended: false }); assert.deepEqual(trace, []);
  if (duringMain) choose([[order, 'absdate'], [teaser, 'on']]);
  const expected = { orderby: duringMain ? 'absdate' : 'r', large: true, extended: duringMain };
  sandbox.load();
  assert.deepEqual(trace, ['thread-controls', ['apply', expected, 'typed search'], ['save', expected]]);
});

test('actual pending catalog submit and reset listeners preserve the ordinary GET actions', () => {
  const form = new EventTarget(), reset = new EventTarget();
  const submit = preferenceSource.slice(preferenceSource.indexOf("  form.addEventListener('submit'"), preferenceSource.indexOf("  if (spoilerControl) spoilers.addEventListener('change'"));
  const resetHandler = preferenceSource.slice(preferenceSource.indexOf("  reset.addEventListener('click'"), preferenceSource.indexOf("  window.addEventListener('storage'"));
  const sandbox = { form, reset, catalogLoaded: false };
  vm.createContext(sandbox); vm.runInContext(submit + resetHandler, sandbox);
  for (const [target, name] of [[form, 'submit'], [reset, 'click']]) {
    const event = new Event(name, { cancelable: true }); target.dispatchEvent(event);
    assert.equal(event.defaultPrevented, false);
  }
});

for (const scenario of ['spoiler', 'clear-search']) test(`actual catalog loader honors an isolated queued ${scenario} choice`, () => {
  const trace = [], value = { orderby: 'alt', large: false, extended: true }, spoilers = new EventTarget();
  spoilers.value = 'off';
  const sandbox = { URL, location: { href: 'https://example.test/fixture/catalog' },
    current: () => ({ ...value }), applyDisplay() {}, initialSpoilers: false, spoilers,
    revealSpoilers: () => spoilers.value === 'on', spoilerControl: true, themePreferences: () => ({}),
    entries: [], key: 'catalog-settings', localStorage: { getItem: () => null },
    stateReady: false, pinKey: 'pins', hideKey: 'hide', pins: new Map(), hiddenThreads: new Map(), readState: () => new Map(),
    searchReady: true, searchControlsReady: false, search: { value: '' }, validQuery: () => true,
    renderedQuery: '', searchKey: 'search', boardKey: 'board', board: 'fixture',
    sessionStorage: { getItem: key => scenario === 'spoiler' ? null : key === 'board' ? 'fixture' : 'previous query' },
    apply: (display, query) => { trace.push(['render', query]); return true; }, updateURL() {},
    saveSpoilers: enabled => trace.push(['spoilers', enabled]), saveSearch: query => trace.push(['search', query]),
  };
  const spoilerHandler = preferenceSource.slice(preferenceSource.indexOf("  if (spoilerControl) spoilers.addEventListener('change'"), preferenceSource.indexOf('  for (const control of [order, size, teaser])'));
  vm.createContext(sandbox);
  vm.runInContext('let catalogLoaded = false, pendingDisplay = null, pendingSearch = null, pendingSpoilers = null;\n' + spoilerHandler + initializers + '\nthis.prepare = prepareCatalog; this.load = loadCatalog;', sandbox);
  sandbox.prepare();
  if (scenario === 'spoiler') { spoilers.value = 'on'; spoilers.dispatchEvent(new Event('change')); }
  else vm.runInContext('pendingSearch = "";', sandbox);
  assert.deepEqual(trace, []); sandbox.load();
  assert.deepEqual(trace, scenario === 'spoiler' ? [['render', ''], ['spoilers', true]] : [['search', '']]);
});
