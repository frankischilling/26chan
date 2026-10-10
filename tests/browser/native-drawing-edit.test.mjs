import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { drawingEditImageUrl, mountDrawingEditLinks } from '../../apps/public/client/native-drawing.js';
import { createDrawingPainter } from '../../apps/public/client/native-drawing-painter.js';

const origin = 'https://media.example.net';
const tick = () => new Promise(resolve => setImmediate(resolve));

test('Edit is reserved for /i/ approved full PNG URLs, including normalized JPEG', () => {
  assert.equal(drawingEditImageUrl(`${origin}/i/42.png`, origin, 'i'), `${origin}/i/42.png`);
  assert.equal(drawingEditImageUrl(`${origin}/i/9007199254740991.png`, origin, 'i'),
    `${origin}/i/9007199254740991.png`);
  for (const board of ['qst', 'vip', 'test', 'I']) {
    assert.equal(drawingEditImageUrl(`${origin}/${board}/42.png`, origin, board), null, board);
  }
  for (const href of [
    `${origin}/i/42.gif`, `${origin}/i/42s.jpg`, `${origin}/i/0.png`,
    `${origin}/i/01.png`, `${origin}/i/9007199254740992.png`,
    `${origin}/vip/42.png`, `${origin}/i/42.png?download=1`, `${origin}/i/42.png#p42`,
    `${origin}/i/42.png/anything`, 'https://evil.example/i/42.png',
    'javascript:alert(1)', `${origin}/i/%34%32.png`, `https://user@${origin.slice(8)}/i/42.png`,
  ]) assert.equal(drawingEditImageUrl(href, origin, 'i'), null, href);
  for (const base of [`${origin}/qst`, `${origin}/?url=1`, 'not a URL', 'file:///tmp', '']) {
    assert.equal(drawingEditImageUrl(`${origin}/i/42.png`, base, 'i'), null, base);
  }
});

function fakeThread() {
  const listeners = new Map(), section = {
    id: 't100', isConnected: true, cells: [],
    querySelectorAll(selector) {
      if (selector === '.fileText') return this.cells;
      if (selector === '[data-drawing-edit-wrap]') return this.cells.flatMap(cell => cell.wrapper ? [cell.wrapper] : []);
      throw new Error(`unexpected selector ${selector}`);
    },
    contains(link) { return this.cells.some(cell => cell.wrapper?.link === link); },
  };
  function file(pid, href, current = section, thumbClass = 'fileThumb') {
    const thumb = { href, className: thumbClass };
    const container = { id: `pc${pid}`, parentElement: current,
      querySelector(selector) { return selector === `#f${pid} a[class="fileThumb"]` && thumb.className === 'fileThumb' ? thumb : null; } };
    const cell = {
      id: `fT${pid}`, wrapper: null, href, thumb,
      closest(selector) { if (selector === '.postContainer') return container; return null; },
      querySelector(selector) {
        if (selector === ':scope > a[href]') return this.href ? { href: this.href } : null;
        if (selector === '[data-drawing-edit-wrap]') return this.wrapper;
        throw new Error(`unexpected selector ${selector}`);
      },
      append(wrapper) {
        this.wrapper = wrapper; wrapper.parentCell = this;
        wrapper.remove = () => { if (this.wrapper === wrapper) this.wrapper = null; };
        if (wrapper.link) wrapper.link.closest = selector =>
          selector === '.fileText' ? this : selector === '[data-drawing-edit]' ? wrapper.link : null;
      },
    };
    section.cells.push(cell); return cell;
  }
  const root = {
    createElement(tag) {
      return {
        tag, dataset: {}, setAttribute(name, value) { this[name] = value; },
        append(...values) { this.link = values.find(value => typeof value === 'object'); },
      };
    },
    addEventListener(name, fn) { if (!listeners.has(name)) listeners.set(name, new Set()); listeners.get(name).add(fn); },
    removeEventListener(name, fn) { listeners.get(name)?.delete(fn); },
    fire(name, event = {}) { for (const fn of listeners.get(name) ?? []) fn(event); },
  };
  return { root, section, file };
}

test('source /i/ Edit links keep the spoiler link visible but require exact fileThumb to import', () => {
  const { root, section, file } = fakeThread(), visited = [];
  const original = file('101', `${origin}/i/81.png`);
  const normalizedJpeg = file('102', `${origin}/i/82.png`);
  const gif = file('103', `${origin}/i/83.gif`);
  file('104', '', section);
  file('105', `${origin}/vip/85.png`);
  file('106', `${origin}/i/86.png`, { id: 't999' });
  const spoiler = file('109', `${origin}/i/89.png`, section, 'fileThumb imgspoiler');
  let allowed = true;
  const edits = mountDrawingEditLinks({
    root, section, board: 'i', mediaOrigin: origin, eligible: () => allowed,
    onEdit: source => visited.push(source),
  });
  assert.equal(original.wrapper.link.textContent, 'Edit');
  assert.equal(normalizedJpeg.wrapper.link.textContent, 'Edit', 'normalized JPEG uses the same approved PNG URL');
  assert.equal(gif.wrapper, null);
  assert.deepEqual(section.cells.slice(3, 6).map(cell => cell.wrapper), [null, null, null]);
  assert.equal(spoiler.wrapper.link.textContent, 'Edit', 'source labels spoiled image Edit even when import is blocked');
  let prevented = 0;
  root.fire('click', { target: original.wrapper.link, preventDefault() { prevented++; } });
  assert.deepEqual(visited, [{ id: '101', url: `${origin}/i/81.png` }]);
  assert.equal(prevented, 1);
  root.fire('click', { target: spoiler.wrapper.link, preventDefault() { prevented++; } });
  assert.equal(prevented, 2); assert.equal(visited.length, 1, 'imgspoiler is not silently imported');
  const later = file('107', `${origin}/i/87.png`);
  root.fire('4chanThreadUpdated'); assert.equal(later.wrapper.link.textContent, 'Edit');
  allowed = false; root.fire('boardThreadStateChanged');
  assert.equal(original.wrapper, null); assert.equal(later.wrapper, null);
  allowed = true; root.fire('boardThreadStateChanged'); assert.ok(later.wrapper);
  const stale = later.wrapper.link;
  later.href = `${origin}/i/87.gif`;
  root.fire('click', { target: stale, preventDefault() {} }); assert.equal(visited.length, 1);
  edits.dispose(); assert.equal(original.wrapper, null);
  assert.equal(later.wrapper, null);
  file('108', `${origin}/i/88.png`);
  root.fire('4chanThreadUpdated'); assert.equal(section.cells.at(-1).wrapper, null);
});

test('qst and vip never mount post Edit links even when ordinary Draw is available', () => {
  for (const board of ['qst', 'vip']) {
    const { root, section, file } = fakeThread();
    const original = file('101', `${origin}/${board}/81.png`), imported = [];
    const links = mountDrawingEditLinks({ root, section, board, mediaOrigin: origin,
      eligible: () => true, onEdit: source => imported.push(source) });
    assert.equal(original.wrapper, null, `${board} must not expose source Edit`);
    assert.equal(links, null);
    assert.deepEqual(imported, []);
  }
});

function fakeEditor() {
  const painter = {
    bg: null, baseWidth: 0, baseHeight: 0, opens: 0, destroys: 0, imports: [],
    open(options) {
      this.opens++; if (this.bg) return;
      assert.equal(options.saveReplay, false); assert.equal(options.replayMode, false);
      this.bg = {}; this.baseWidth = options.width; this.baseHeight = options.height;
      this.onDoneCb = options.onDone; this.onCancelCb = options.onCancel;
    },
    destroy() { this.destroys++; this.bg = null; },
    hide() {},
    flatten() { return { width: this.baseWidth, height: this.baseHeight }; },
    resizeCanvas(width, height) { this.baseWidth = width; this.baseHeight = height; },
  };
  painter.onOpenImageLoaded = function() {
    painter.imports.push({ url: this.src, crossOrigin: this.crossOrigin, width: this.naturalWidth, height: this.naturalHeight });
    painter.resizeCanvas(this.naturalWidth, this.naturalHeight);
  };
  return painter;
}
function fakeClient(key = 'quick-reply', target = '100') {
  const state = { disposed: false, allowed: true, pending: false, loading: false, prepared: 0, replaces: 0, files: [], errors: [] };
  return {
    key, target, state, disposed: () => state.disposed, allowed: () => state.allowed,
    pending: () => state.pending, loading: value => { state.loading = value; },
    error: error => { state.errors.push(error); },
    async prepare() { state.prepared++; return true; },
    async clearForReplacement() { state.pending = false; state.replaces++; return true; },
    replaced() { state.pending = false; }, exporting() { state.pending = true; },
    async finished(file) { state.files.push(file); },
    async cancelled() { state.pending = false; },
  };
}
function fakeImages() {
  const images = [];
  return {
    images, createImage() {
      const image = { naturalWidth: 32, naturalHeight: 24, src: '', onload: null, onerror: null };
      images.push(image); return image;
    },
  };
}
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test('anonymous source image enters Tegaki only after a bounded CORS image load, then Finish uses existing PNG exporter', async () => {
  const painter = fakeEditor(), network = fakeImages(), owner = fakeClient();
  let exports = 0;
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage(),
    exportFile: async canvas => { exports++; return { name: 'tegaki.png', width: canvas.width, height: canvas.height }; } });
  const url = `${origin}/qst/81.png`, opened = drawing.importFromPost(owner, url);
  assert.equal(network.images.length, 1);
  assert.equal(network.images[0].crossOrigin, 'anonymous');
  assert.equal(network.images[0].referrerPolicy, 'no-referrer');
  assert.equal(network.images[0].src, url);
  assert.equal(owner.state.loading, true);
  assert.equal(painter.opens, 0, 'no editor or canvas allocation before image qualification');
  network.images[0].onload();
  assert.equal(await opened, true);
  assert.equal(owner.state.loading, false);
  assert.deepEqual(painter.imports, [{ url, crossOrigin: 'anonymous', width: 32, height: 24 }]);
  await painter.onDoneCb();
  assert.deepEqual(owner.state.files, [{ name: 'tegaki.png', width: 32, height: 24 }]);
  assert.equal(exports, 1);
  drawing.invalidate(owner);
  assert.equal(painter.destroys, 0, 'Finish/Clear keep the one retained Tegaki canvas');
  drawing.dispose();
});

test('imported source ID and rounded elapsed seconds survive retained Done/Edit but change on a fresh import', async () => {
  const painter = fakeEditor(), network = fakeImages(), owner = fakeClient();
  let now = 3500, importStart = 1000, sourceId = null;
  const completed = [];
  painter.onOpenImageLoaded = function() {
    painter.imports.push(this.src); painter.resizeCanvas(this.naturalWidth, this.naturalHeight);
    painter.hasCustomCanvas = true; painter.startTimeStamp = importStart;
  };
  owner.imported = id => { sourceId = id; };
  owner.sourcePost = () => sourceId;
  owner.replaced = () => { sourceId = null; };
  owner.finished = async (file, seconds) => { completed.push({ sourceId, seconds }); };
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage(),
    now: () => now, confirmImportReplacement: () => true, exportFile: async () => ({ name: 'tegaki.png' }) });

  const first = drawing.importFromPost(owner, { id: '101', url: `${origin}/i/501.png` });
  network.images[0].onload();
  assert.equal(await first, true);
  assert.equal(sourceId, '101');
  await painter.onDoneCb();
  assert.deepEqual(completed, [{ sourceId: '101', seconds: 3 }]);

  now = 6400;
  assert.equal(await drawing.open(owner, 400, 400), true, 'reopen retained imported layers');
  await painter.onDoneCb();
  assert.deepEqual(completed.at(-1), { sourceId: '101', seconds: 5 }, 'elapsed time accumulates from import');

  now = 12500; importStart = 12000;
  const next = drawing.importFromPost(owner, { id: '102', url: `${origin}/i/502.png` });
  network.images[1].onload();
  assert.equal(await next, true);
  assert.equal(sourceId, '102', 'new imported post owns subsequent annotation');
  await painter.onDoneCb();
  assert.deepEqual(completed.at(-1), { sourceId: '102', seconds: 1 });
  drawing.dispose();
});

test('bad CORS response, oversized source, failed image and timed-out load never change retained canvas or receipt', async () => {
  for (const failure of ['network', 'oversized', 'timeout']) {
    const painter = fakeEditor(), network = fakeImages(), owner = fakeClient();
    const timers = [];
    const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage(),
      schedule: callback => { timers.push(callback); return timers.length; }, unschedule: () => {} });
    await drawing.open(owner, 400, 400);
    await painter.onDoneCb();
    owner.state.pending = true;
    const oldCanvas = painter.bg, oldWidth = painter.baseWidth, oldLoads = painter.opens;
    const request = drawing.importFromPost(owner, `${origin}/qst/81.png`);
    if (failure === 'network') network.images[0].onerror();
    if (failure === 'oversized') { network.images[0].naturalWidth = 1025; network.images[0].onload(); }
    if (failure === 'timeout') timers[0]();
    assert.equal(await request, false, failure);
    assert.equal(network.images[0].src, '');
    assert.equal(painter.bg, oldCanvas);
    assert.equal(painter.baseWidth, oldWidth);
    assert.equal(painter.opens, oldLoads);
    assert.equal(owner.state.prepared, 1, 'failed import never cancels earlier receipt');
    assert.equal(owner.state.pending, true);
    assert.match(owner.state.errors.at(-1), /source image could not be loaded/);
    drawing.dispose();
  }
});

test('double-click is single-flight and stale load cannot open after close, retarget, replacement or pagehide', async () => {
  for (const action of ['close', 'retarget', 'pagehide', 'disabled']) {
    const painter = fakeEditor(), network = fakeImages(), first = fakeClient(), next = fakeClient('ordinary', '0');
    const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage() });
    const pending = drawing.importFromPost(first, `${origin}/qst/81.png`);
    assert.equal(await drawing.importFromPost(first, `${origin}/qst/81.png`), false, 'double-click has no second request');
    assert.equal(network.images.length, 1);
    const late = network.images[0].onload;
    if (action === 'close') { drawing.invalidate(first); first.state.disposed = true; }
    if (action === 'retarget') { drawing.invalidate(first); first.state.disposed = true; await drawing.open(next, 400, 400); }
    if (action === 'pagehide') drawing.dispose();
    if (action === 'disabled') { first.state.allowed = false; }
    late();
    assert.equal(await pending, false, action);
    assert.equal(painter.imports.length, 0);
    assert.equal(painter.opens, action === 'retarget' ? 1 : 0);
    if (action !== 'pagehide') drawing.dispose();
  }
});

test('Clear fences module loading and upload cancellation after a valid source image was decoded', async () => {
  for (const stage of ['module', 'preparation']) {
    const painter = fakeEditor(), network = fakeImages(), owner = fakeClient(), work = deferred();
    if (stage === 'preparation') owner.prepare = () => work.promise;
    const drawing = createDrawingPainter({
      load: stage === 'module' ? () => work.promise : async () => painter,
      createImage: () => network.createImage(),
    });
    const pending = drawing.importFromPost(owner, `${origin}/qst/81.png`);
    network.images[0].onload();
    await tick();
    drawing.invalidate(owner);
    work.resolve(stage === 'module' ? painter : true);
    assert.equal(await pending, false, stage);
    assert.equal(painter.opens, 0, `${stage}: stale completion cannot create Tegaki`);
    assert.equal(painter.imports.length, 0);
    drawing.dispose();
  }
});

test('a stale open cannot clear loading while a newer import is pending for the same form', async () => {
  const painter = fakeEditor(), network = fakeImages(), owner = fakeClient(), stalled = deferred();
  let preparations = 0;
  owner.prepare = () => ++preparations === 1 ? stalled.promise : Promise.resolve(true);
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage() });
  const obsolete = drawing.open(owner, 400, 400);
  await tick();
  assert.equal(owner.state.loading, true);
  drawing.invalidate(owner);
  assert.equal(owner.state.loading, false, 'invalidating an attempt immediately restores the controls');
  const current = drawing.importFromPost(owner, `${origin}/qst/91.png`);
  assert.equal(owner.state.loading, true);
  stalled.resolve(true);
  assert.equal(await obsolete, false);
  assert.equal(owner.state.loading, true, 'the old finally cannot enable the file input during a new load');
  assert.equal(painter.opens, 0);
  network.images[0].onload();
  assert.equal(await current, true);
  assert.equal(owner.state.loading, false);
  assert.equal(painter.opens, 1);
  drawing.dispose();
});

test('invalidating the retained owner clears a different pending replacement without unlocking its successor', async () => {
  const painter = fakeEditor(), owner = fakeClient('ordinary', '0');
  const obsoleteClient = fakeClient('quick-reply', '100'), nextClient = fakeClient('quick-reply', '101');
  const oldCancellation = deferred(), nextPreparation = deferred();
  let cancellations = 0;
  owner.clearForReplacement = () => ++cancellations === 1 ? oldCancellation.promise : Promise.resolve(true);
  nextClient.prepare = () => nextPreparation.promise;
  const drawing = createDrawingPainter({ load: async () => painter, confirmReplacement: () => true,
    exportFile: async () => ({ name: 'tegaki.png' }) });
  assert.equal(await drawing.open(owner, 400, 400), true);
  await painter.onDoneCb();
  const canvas = painter.bg;
  const obsolete = drawing.open(obsoleteClient, 300, 300);
  await tick();
  assert.equal(obsoleteClient.state.loading, true, 'replacement waits for the previous owner to clear');
  drawing.invalidate(owner);
  assert.equal(obsoleteClient.state.loading, false, 'invalidated replacement immediately releases its own spinner');
  assert.equal(painter.bg, canvas, 'invalidating an open must retain the existing canvas');

  const current = drawing.open(nextClient, 200, 200);
  await tick();
  assert.equal(nextClient.state.loading, true);
  oldCancellation.resolve(true);
  assert.equal(await obsolete, false);
  assert.equal(nextClient.state.loading, true, 'obsolete completion cannot unlock a newer replacement');
  nextPreparation.resolve(true);
  assert.equal(await current, true);
  assert.equal(nextClient.state.loading, false);
  assert.equal(painter.opens, 2, 'only the original canvas and final replacement open');
  assert.equal(cancellations, 2);
  drawing.dispose();
});

test('suspending a page fences a decoded source and preserves its retained Tegaki layers', async () => {
  const painter = fakeEditor(), network = fakeImages(), owner = fakeClient();
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage() });
  assert.equal(await drawing.open(owner, 400, 400), true);
  await painter.onDoneCb();
  const canvas = painter.bg;
  const pending = drawing.importFromPost(owner, `${origin}/qst/92.png`);
  const late = network.images[0].onload;
  drawing.suspend();
  assert.equal(drawing.active(), false);
  assert.equal(owner.state.loading, false);
  assert.equal(network.images[0].src, '');
  late();
  assert.equal(await pending, false);
  assert.equal(painter.bg, canvas);
  assert.equal(painter.destroys, 0);
  assert.equal(await drawing.open(owner, 400, 400), true, 'restored BFCache state can reopen the same canvas');
  assert.equal(painter.bg, canvas);
  assert.equal(painter.destroys, 0);
  drawing.dispose();
  assert.equal(painter.destroys, 1, 'real unload still destroys the retained engine');
});

test('a reopened QR controller can identify and discard the same-target canvas it did not create', async () => {
  const painter = fakeEditor(), old = fakeClient('quick-reply', '100');
  const drawing = createDrawingPainter({ load: async () => painter });
  assert.equal(await drawing.open(old, 400, 400), true);
  await painter.onDoneCb();
  drawing.invalidate(old);
  old.state.disposed = true;
  const reopened = fakeClient('quick-reply', '100');
  assert.equal(drawing.retained('quick-reply', '100'), true);
  assert.equal(drawing.retained('quick-reply', '101'), false);
  assert.equal(drawing.retained('ordinary', '100'), false);
  drawing.invalidate(reopened, { destroy: true });
  assert.equal(painter.destroys, 1);
  assert.equal(drawing.retained('quick-reply', '100'), false);
  drawing.dispose();
  assert.equal(painter.destroys, 1);
});

test('QR close followed by a different thread never silently replaces retained unsent layers', async () => {
  const painter = fakeEditor(), first = fakeClient('quick-reply', '100');
  let allowed = false, questions = 0;
  const drawing = createDrawingPainter({ load: async () => painter,
    confirmReplacement: () => { questions++; return allowed; }, });
  assert.equal(await drawing.open(first, 400, 400), true);
  await painter.onDoneCb();
  drawing.invalidate(first);
  first.state.disposed = true;
  first.state.pending = false;
  const canvas = painter.bg, newTarget = fakeClient('quick-reply', '101');
  assert.equal(await drawing.open(newTarget, 300, 300), false);
  assert.equal(questions, 1);
  assert.equal(painter.bg, canvas);
  assert.equal(painter.destroys, 0);
  assert.equal(newTarget.state.prepared, 0);
  allowed = true;
  assert.equal(await drawing.open(newTarget, 300, 300), true);
  assert.equal(questions, 2);
  assert.equal(painter.destroys, 1);
  assert.equal(painter.baseWidth, 300);
  drawing.dispose();
});

test('source Edit import requires confirmation even after form Clear reset its approval', async () => {
  const painter = fakeEditor(), images = fakeImages(), owner = fakeClient();
  let confirmed = false;
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => images.createImage(),
    confirmImportReplacement: () => confirmed });
  assert.equal(await drawing.open(owner, 400, 400), true);
  await painter.onDoneCb();
  owner.state.pending = false;
  const canvas = painter.bg;
  const declined = drawing.importFromPost(owner, `${origin}/qst/92.png`);
  images.images[0].onload();
  assert.equal(await declined, false);
  assert.equal(painter.bg, canvas);
  assert.equal(painter.destroys, 0);
  confirmed = true;
  const accepted = drawing.importFromPost(owner, `${origin}/qst/93.png`);
  images.images[1].onload();
  assert.equal(await accepted, true);
  assert.equal(painter.destroys, 1);
  assert.equal(painter.imports.length, 1);
  drawing.dispose();
});

test('persisted pagehide suspends drawing and retains only an idle approved receipt; real unload disposes', async () => {
  const source = await readFile(new URL('../../apps/public/client/native-drawing.js', import.meta.url), 'utf8');
  const start = source.indexOf('  function onPageHide(event) {');
  const end = source.indexOf("  window.addEventListener('pagehide', onPageHide);", start);
  assert.ok(start > 0 && end > start, 'use the installed page lifecycle handlers');
  const handlers = source.slice(start, end);

  function state({ approved = true, posting = false } = {}) {
    const calls = [];
    const context = {
      dead: false, suspended: false, busy: posting, postEpoch: 5,
      postController: { abort() { calls.push('abort-post'); } },
      ordinary: { dispose() { calls.push('dispose-controls'); }, sync() { calls.push('sync-controls'); } },
      transfer: {
        snapshot: () => ({ pending: true, approved }),
        resetTarget() { calls.push('reset-target'); },
        retire() { calls.push('retire-one-use'); },
        dispose() { calls.push('dispose-upload'); },
      },
      painter: { suspend() { calls.push('suspend-canvas'); }, dispose() { calls.push('destroy-canvas'); } },
      status: { textContent: '' }, sync() { calls.push('sync-buttons'); },
    };
    vm.runInNewContext(handlers, context, { timeout: 100 });
    return { context, calls };
  }

  const stable = state(), { context, calls } = stable;
  context.onPageHide({ persisted: true });
  assert.equal(context.suspended, true);
  assert.equal(context.dead, false);
  assert.equal(context.postEpoch, 6);
  assert.equal(context.postController, null);
  assert.ok(calls.includes('suspend-canvas'));
  assert.ok(!calls.includes('reset-target') && !calls.includes('retire-one-use'));
  assert.ok(!calls.includes('destroy-canvas'), 'BFCache keeps the canvas');
  context.onPageShow({ persisted: true });
  assert.equal(context.suspended, false);
  assert.ok(calls.filter(value => value === 'sync-controls').length >= 2);
  context.onPageHide({ persisted: false });
  assert.equal(context.dead, true);
  assert.ok(calls.includes('dispose-controls') && calls.includes('dispose-upload') && calls.includes('destroy-canvas'));
  context.onPageShow({ persisted: true });
  assert.equal(context.dead, true, 'actual unload can never rearm a disposed controller');

  const interrupted = state({ approved: false });
  interrupted.context.onPageHide({ persisted: true });
  assert.ok(interrupted.calls.includes('reset-target'));
  assert.ok(!interrupted.calls.includes('retire-one-use'));
  assert.ok(!interrupted.calls.includes('destroy-canvas'));

  const posting = state({ approved: true, posting: true });
  posting.context.onPageHide({ persisted: true });
  assert.ok(posting.calls.includes('retire-one-use'));
  assert.ok(!posting.calls.includes('reset-target'));
  assert.equal(posting.context.busy, false);
  assert.match(posting.context.status.textContent, /result uncertain/);
});

test('import replacement waits for cancellation, respects refusal and never reuses an old approved drawing', async () => {
  const painter = fakeEditor(), network = fakeImages(), owner = fakeClient();
  const drawing = createDrawingPainter({ load: async () => painter, createImage: () => network.createImage(),
    confirmImportReplacement: () => false });
  await drawing.open(owner, 400, 400);
  await painter.onDoneCb();
  owner.state.pending = true;
  const oldCanvas = painter.bg;
  const refused = drawing.importFromPost(owner, `${origin}/qst/81.png`);
  network.images[0].onload();
  assert.equal(await refused, false);
  assert.equal(painter.bg, oldCanvas);
  assert.equal(painter.destroys, 0);
  assert.equal(owner.state.prepared, 1);
  drawing.dispose();
  const acceptedEditor = fakeEditor(), another = fakeImages(), newOwner = fakeClient();
  const confirmed = createDrawingPainter({ load: async () => acceptedEditor, createImage: () => another.createImage(),
    confirmImportReplacement: () => true });
  await confirmed.open(newOwner, 400, 400);
  await acceptedEditor.onDoneCb();
  newOwner.state.pending = true;
  const editing = confirmed.importFromPost(newOwner, `${origin}/qst/82.png`);
  another.images[0].onload(); assert.equal(await editing, true);
  assert.equal(newOwner.state.replaces, 1);
  assert.equal(newOwner.state.prepared, 2);
  assert.equal(acceptedEditor.destroys, 1);
  assert.equal(acceptedEditor.opens, 2);
  confirmed.dispose();
});

test('import keeps the previous canvas when upload cancellation fails and cleans the prior owner before cross-form replacement', async () => {
  const painter = fakeEditor(), network = fakeImages(), old = fakeClient(), next = fakeClient('quick-reply', '100');
  let allowCancel = false;
  const drawing = createDrawingPainter({
    load: async () => painter, createImage: () => network.createImage(),
    confirmImportReplacement: () => true,
  });
  await drawing.open(old, 400, 400);
  await painter.onDoneCb(); old.state.pending = true;
  const canvas = painter.bg;
  old.clearForReplacement = async () => { old.state.replaces++; if (!allowCancel) return false; old.state.pending = false; return true; };
  const denied = drawing.importFromPost(next, `${origin}/qst/81.png`);
  network.images[0].onload();
  assert.equal(await denied, false);
  assert.equal(painter.bg, canvas);
  assert.equal(painter.destroys, 0);
  assert.equal(next.state.prepared, 0);
  allowCancel = true;
  const accepted = drawing.importFromPost(next, `${origin}/qst/82.png`);
  network.images[1].onload(); assert.equal(await accepted, true);
  assert.equal(old.state.replaces, 2);
  assert.equal(next.state.prepared, 1);
  assert.equal(painter.destroys, 1);
  assert.equal(painter.opens, 2);
  assert.equal(painter.imports.length, 1);
  drawing.dispose();
});
