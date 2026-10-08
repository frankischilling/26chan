import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import { createInitialMountLifecycle, dispatchSourceEvent } from '../../apps/public/client/native-source-events.js';

const oracle = JSON.parse(await readFile(new URL('../fixtures/native-main-init-source.json', import.meta.url)));
const snippets = Object.fromEntries(Object.entries(oracle.snippets).map(([name, row]) => [name, row.text]));
function documentFixture() {
  const document = new EventTarget();
  document.createEvent = kind => {
    assert.equal(kind, 'Event');
    const event = new Event('4chanMainInit');
    event.initEvent = (name, bubbles, cancelable) => {
      assert.equal(name, '4chanMainInit'); assert.equal(bubbles, false); assert.equal(cancelable, false);
    };
    return event;
  };
  return document;
}
function transition(window, name, persisted = true) {
  const event = new Event(name); event.persisted = persisted; window.dispatchEvent(event);
}
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const tick = async () => { await Promise.resolve(); await Promise.resolve(); };

for (const disableAll of [false, true]) test(`actual pinned Main.init publishes prepared context before parser startup (disabled=${disableAll})`, () => {
  assert.equal(oracle.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  for (const row of Object.values(oracle.snippets)) {
    assert.equal(Buffer.byteLength(row.text), row.byte_end - row.byte_start);
    assert.equal(createHash('sha256').update(row.text).digest('hex'), row.sha256);
  }
  const order = [], document = documentFixture();
  const Main = { run() { order.push('run'); }, getCookie: () => null,
    initIcons: () => order.push('icons'), addCSS: () => order.push('css') };
  const sandbox = { document, Main, UA: { init: () => order.push('ua') },
    Config: { disableAll, load: () => order.push('settings') }, QR: {}, window: { passEnabled: false },
    location: { host: 'boards.4chan.org', pathname: '/sci/thread/100', protocol: 'https:' }, style_group: 'ws_style' };
  document.addEventListener('4chanMainInit', event => {
    assert.equal(event.constructor, Event); assert.equal(event.bubbles, false); assert.equal(event.cancelable, false);
    assert.equal(event.target, document); assert.equal(Object.hasOwn(event, 'detail'), false);
    assert.equal(Main.board, 'sci'); assert.equal(Main.page, 'thread'); assert.equal(Main.tid, '100');
    assert.equal(Main.stylesheet, 'yotsuba_b_new'); assert.equal(Main.type, 'ws'); order.push('main');
  });
  vm.createContext(sandbox); vm.runInContext(snippets.dispatch + snippets.init, sandbox, { timeout: 100 });
  sandbox.Main.init();
  assert.deepEqual(order, ['ua', 'settings', 'icons', 'css', 'main']);
  document.dispatchEvent(new Event('DOMContentLoaded'));
  assert.equal(order.at(-1), 'run');
  assert.ok(snippets.run.indexOf('if (Config.disableAll)') < snippets.run.indexOf('Parser.init()'));
  assert.ok(snippets.run.indexOf('Parser.init()') < snippets.run.indexOf('Parser.parseThread(Main.tid)'));
  assert.match(snippets.math_startup, /window\.math_tags && pageHasMath\(\)/);
  assert.doesNotMatch(snippets.math_startup, /Config|disableAll/);
});

test('native MainInit uses the same payload-free event shape', () => {
  const document = documentFixture(); let count = 0;
  document.addEventListener('4chanMainInit', event => {
    count++; assert.equal(event.target, document); assert.equal(event.constructor, Event);
    assert.equal(event.bubbles, false); assert.equal(event.cancelable, false); assert.equal(Object.hasOwn(event, 'detail'), false);
  });
  dispatchSourceEvent(document, '4chanMainInit'); assert.equal(count, 1);
});

function bootstrap() {
  const window = new EventTarget(), lifecycle = createInitialMountLifecycle(window), imported = deferred();
  const order = ['settings', 'main']; let mounts = 0;
  const pending = (async () => {
    await imported.promise;
    while (!lifecycle.active()) {
      if (!await lifecycle.wait()) { lifecycle.disconnect(); return false; }
    }
    lifecycle.disconnect(); mounts++; order.push('mount'); return true;
  })();
  return { window, lifecycle, imported, pending, order, mounts: () => mounts };
}

for (const restoredBeforeImport of [false, true]) test(`pending import resumes once across BFcache (restore first=${restoredBeforeImport})`, async () => {
  const h = bootstrap(); transition(h.window, 'pagehide');
  if (restoredBeforeImport) transition(h.window, 'pageshow');
  h.imported.resolve(); await tick();
  if (!restoredBeforeImport) { assert.equal(h.mounts(), 0); transition(h.window, 'pageshow'); }
  assert.equal(await h.pending, true); assert.equal(h.mounts(), 1);
  transition(h.window, 'pageshow'); transition(h.window, 'pagehide'); transition(h.window, 'pageshow');
  await tick(); assert.equal(h.mounts(), 1); assert.deepEqual(h.order, ['settings', 'main', 'mount']);
});

test('a second pagehide between wake and continuation keeps the mount suspended', async () => {
  const h = bootstrap(); transition(h.window, 'pagehide'); h.imported.resolve(); await tick();
  transition(h.window, 'pageshow'); transition(h.window, 'pagehide'); await tick();
  assert.equal(h.mounts(), 0); transition(h.window, 'pageshow');
  assert.equal(await h.pending, true); assert.equal(h.mounts(), 1);
});

for (const importFirst of [false, true]) test(`terminal departure cancels pending mounting (import first=${importFirst})`, async () => {
  const h = bootstrap(); transition(h.window, 'pagehide');
  if (importFirst) { h.imported.resolve(); await tick(); }
  transition(h.window, 'pagehide', false); transition(h.window, 'pageshow'); h.imported.resolve();
  assert.equal(await h.pending, false); assert.equal(h.mounts(), 0);
});

test('unrelated public notifications and nonpersisted pageshow cannot open the private gate', async () => {
  const h = bootstrap(); transition(h.window, 'pagehide'); h.imported.resolve(); await tick();
  for (const type of ['4chanMainInit', '4chanParsingDone', '4chanSettingsSaved']) h.window.dispatchEvent(new Event(type));
  transition(h.window, 'pageshow', false); await tick(); assert.equal(h.mounts(), 0);
  transition(h.window, 'pageshow'); assert.equal(await h.pending, true);
});

test('importing the actual math module has no page-mount side effects', async () => {
  const saved = Object.getOwnPropertyDescriptor(globalThis, 'document'); let queries = 0;
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    body: { dataset: { mathTags: '1' } }, querySelector() { queries++; return null; },
  } });
  try {
    const math = await import('../../apps/public/client/native-math.js?main-init-inert');
    assert.equal(queries, 0); assert.equal(math.pageNativeMath(), null); assert.equal(queries, 1);
  } finally { if (saved) Object.defineProperty(globalThis, 'document', saved); else delete globalThis.document; }
});

const watcherSource = (await readFile(new URL('../../apps/public/static/thread-watcher.v1.js', import.meta.url), 'utf8'))
  .replace(/^import[\s\S]*?;\n/gm, '');
for (const kind of ['thread', 'board', 'disabled', 'catalog', 'upload', 'interrupted', 'departed']) test(`actual watcher prepares settings before MainInit and projection (${kind})`, async () => {
  const order = [], document = documentFixture(), window = new EventTarget();
  document.getElementById = () => null; document.body = { dataset: {} };
  document.querySelector = selector => selector === '.board' && kind !== 'catalog' && kind !== 'upload' ? {} : null;
  const stop = new Error('stop after verified bootstrap boundary');
  const config = kind === 'disabled' ? { disableAll: true } : { threadWatcher: true };
  document.addEventListener('4chanMainInit', () => {
    assert.ok(order.includes('settings-read')); assert.ok(order.includes('presentation')); order.push('main');
    if (kind === 'interrupted' || kind === 'departed') transition(window, 'pagehide', kind === 'interrupted');
  });
  const sandbox = {
    document, window, AbortController, navigator: { userAgent: '' },
    createInitialMountLifecycle, createParsingBootstrap: () => ({}), dispatchSourceEvent,
    NativeWatchLock: class {}, postId: id => id === '0' ? null : id,
    readWatches: () => new Map(), sourceMobileLayout: value => value,
    matchMedia: () => ({ matches: false }),
    localStorage: { getItem: key => { if (key === '4chan-settings') { order.push('settings-read'); return JSON.stringify(config); } return null; } },
    captureSettingsPresentation: () => { order.push('presentation'); return { firstRun: false }; },
    createCommentProjection: () => { order.push('projection'); throw stop; },
  };
  vm.createContext(sandbox); vm.runInContext(watcherSource, sandbox, { timeout: 100 });
  const pending = sandbox.start({ dataset: { board: 'sci', thread: kind === 'board' ? '0' : '100', catalog: String(kind === 'catalog') } });
  if (kind === 'interrupted') {
    await tick(); assert.equal(order.includes('projection'), false); transition(window, 'pageshow');
  }
  if (kind === 'departed') {
    assert.equal(await pending, undefined); transition(window, 'pageshow'); await tick();
    assert.equal(order.includes('projection'), false);
  } else await assert.rejects(pending, error => error === stop);
  assert.equal(order.filter(value => value === 'main').length, kind === 'catalog' || kind === 'upload' ? 0 : 1);
  if (order.includes('main') && kind !== 'departed') assert.ok(order.indexOf('main') < order.indexOf('projection')); 
});
