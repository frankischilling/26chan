// Source/preparation/ownership checks ONLY. No Node result proves native pixels.
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { CASES, encodeOwnedCase } from '../fixtures/replay-worker/cases.mjs';
import { SOURCE, build, expectedFiles } from '../fixtures/replay-worker/build.mjs';
import { decodeReplayCandidateWire, prepareReplayCandidate } from '../../apps/public/client/native-replay-data.js';
import { createInertReplayRuntime } from '../helpers/pinned-replay-runtime.mjs';
import { WorkerCommandGate, exactMessage } from '../fixtures/replay-worker/protocol.mjs';
import { ProbeController } from '../fixtures/replay-worker/controller.mjs';
import { installWorkerBoundary } from '../fixtures/replay-worker/worker-boundary.mjs';
import { staticFiles, makeHandler } from '../fixtures/replay-worker/serve.mjs';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

test('generated consumer pins entire production source and separate adapter without mutation', async () => {
  assert.equal(await build(true), 24);
  const files = await expectedFiles(), consumer = files.get('consumer-v1.mjs');
  assert.equal(hash(consumer.subarray(0, SOURCE.bytes)), SOURCE.sha256);
  const adapter = await readFile(new URL('../fixtures/replay-worker/consumer-adapter.inc.js', import.meta.url));
  assert(consumer.subarray(-adapter.length).equals(adapter));
  assert(!adapter.includes(Buffer.from('eval(')));
  const changed = Buffer.from(consumer); changed[100] ^= 1;
  assert.notEqual(hash(changed.subarray(0, SOURCE.bytes)), SOURCE.sha256);
  const manifest = JSON.parse(files.get('manifest.json'));
  assert.equal(manifest.adapter.sha256, hash(adapter));
  assert.equal(manifest.artifacts['consumer-v1.mjs'].sha256, hash(consumer));
});
test('all owned cases structurally prepare through pinned constructors without runtime activation', async () => {
  const { viewer, calls } = await createInertReplayRuntime();
  const seenTools = new Set(), tips = new Set();
  for (const item of CASES) {
    const candidate = decodeReplayCandidateWire(encodeOwnedCase(item));
    const prepared = prepareReplayCandidate(candidate, viewer);
    assert.equal(prepared.events.length, item.events.length);
    assert.equal(candidate.metadata.width, item.width); assert.equal(candidate.metadata.height, item.height);
    assert.equal(prepared.metadata.toolId, item.toolId);
    assert.equal(Object.keys(prepared.toolMap).length, 8);
    for (let i = 0; i < item.events.length; i++) {
      assert.equal(prepared.events[i].type, item.events[i][0]);
      assert.equal(prepared.events[i].timeStamp, i * 7);
      assert(Object.isFrozen(prepared.events[i]));
      if (item.events[i][0] === 10) seenTools.add(item.events[i][1]);
      if (item.events[i][0] === 15) tips.add(item.events[i][1]);
    }
  }
  assert.deepEqual([...seenTools].sort((a, b) => a - b), [1, 2, 3, 5, 7, 8]);
  assert.deepEqual([...tips], [0, 1, 2]); assert.deepEqual(calls, []);
});
test('whole-source module import is explicitly ordered after worker document installation', async () => {
  const worker = await readFile(new URL('../fixtures/replay-worker/worker.mjs', import.meta.url), 'utf8');
  assert(worker.indexOf('installWorkerBoundary();') < worker.indexOf("await import('./generated/consumer-v1.mjs')"));
  assert(!/^import .*consumer-v1/m.test(worker));
  assert.match(worker, /postMessage\([\s\S]*\}, \[bitmap\]\)/);
  assert.doesNotMatch(worker, /transferToImageBitmap\(\).*activeLayer|\[.*\.buffer\]/);
});
test('gate rejects stale generations, duplicate commands, premature steps and wrong acknowledgements', () => {
  const gate = new WorkerCommandGate();
  assert.throws(() => gate.begin({ generation: 1, sequence: 1, type: 'step' }), /generation/);
  gate.initialize(3);
  assert.throws(() => gate.initialize(4), /initialization/);
  assert.throws(() => gate.begin({ generation: 2, sequence: 1, type: 'initial' }), /generation/);
  gate.begin({ generation: 3, sequence: 1, type: 'initial' });
  assert.throws(() => gate.begin({ generation: 3, sequence: 2, type: 'step' }), /acknowledgement/);
  assert.throws(() => gate.acknowledge({ generation: 3, sequence: 2 }), /acknowledgement/);
  assert.throws(() => gate.acknowledge({ generation: 2, sequence: 1 }), /acknowledgement/);
  gate.acknowledge({ generation: 3, sequence: 1 });
  assert.throws(() => gate.begin({ generation: 3, sequence: 1, type: 'step' }), /sequence/);
  gate.begin({ generation: 3, sequence: 2, type: 'step' }); gate.close();
  assert.throws(() => gate.acknowledge({ generation: 3, sequence: 2 }), /acknowledgement/);
  assert.throws(() => exactMessage({ type: 'ack', extra: true }, ['type']), /record/);
});

class FakeWorker {
  static instances = [];
  constructor(url) { this.url = String(url); this.messages = []; this.terminated = false; FakeWorker.instances.push(this); }
  postMessage(message) { this.messages.push(message); }
  terminate() { this.terminated = true; }
  emit(message) { this.onmessage({ data: message }); }
}
class ManualTimers {
  next = 0; tasks = new Map();
  setTimeout = fn => { const id = ++this.next; this.tasks.set(id, fn); return id; };
  clearTimeout = id => this.tasks.delete(id);
  fire() { const tasks = [...this.tasks.values()]; this.tasks.clear(); for (const fn of tasks) fn(); }
}
const fakeBitmap = () => ({ width: 1, height: 1, closed: 0, close() { this.closed++; } });
// This fake exists only for controller ownership tests, never raster comparison.
const fakeCanvas = () => ({ getContext: () => ({ drawImage() {}, getImageData: () => ({ data: [1, 2, 3, 255] }) }) });
async function ready(controller) {
  const promise = controller.start('pencil-pressure'), worker = FakeWorker.instances.at(-1);
  const generation = worker.messages[0].generation;
  worker.emit({ type: 'ready', generation, sequence: 0, eventCount: 5 }); await promise;
  return { worker, generation };
}
test('parent protects startup before Worker construction and timeout terminates independently', async () => {
  const timers = new ManualTimers();
  class CheckedWorker extends FakeWorker { constructor(url) { assert.equal(timers.tasks.size, 1); super(url); } }
  const c = new ProbeController(fakeCanvas(), { WorkerClass: CheckedWorker, timers });
  const promise = c.start('pencil-pressure'), failure = assert.rejects(promise, /deadline/);
  timers.fire(); await failure;
  assert.equal(c.entry, null); assert.equal(FakeWorker.instances.at(-1).terminated, true);
});
test('one command/frame, bitmap disposal, no idle production and exact ack ownership', async () => {
  const timers = new ManualTimers(), c = new ProbeController(fakeCanvas(), { WorkerClass: FakeWorker, timers });
  const { worker, generation } = await ready(c);
  const promise = c.initial(), bitmap = fakeBitmap();
  worker.emit({ type: 'frame', generation, sequence: 1, bitmap }); await promise;
  assert.equal(bitmap.closed, 1); assert.equal(c.stats.presented, 1); assert.equal(timers.tasks.size, 1);
  await assert.rejects(c.step(), /acknowledgement/);
  const ack = c.acknowledge(); assert.equal(timers.tasks.size, 1);
  worker.emit({ type: 'acked', generation, sequence: 1 }); await ack;
  assert.equal(timers.tasks.size, 0); assert.equal(worker.messages.length, 3); c.close();
});
test('missing acknowledgement timeout retires worker and closes late native-ownership messages', async () => {
  const timers = new ManualTimers(), c = new ProbeController(fakeCanvas(), { WorkerClass: FakeWorker, timers });
  const { worker, generation } = await ready(c);
  const initial = c.initial(), bitmap = fakeBitmap(); worker.emit({ type: 'frame', generation, sequence: 1, bitmap }); await initial;
  timers.fire(); assert.equal(worker.terminated, true); assert.equal(c.entry, null);
  const late = fakeBitmap(); worker.emit({ type: 'frame', generation, sequence: 2, bitmap: late });
  assert.equal(late.closed, 1); assert.equal(c.stats.presented, 1);
});
test('replacement cannot revive stale generation and closes rejected bitmaps', async () => {
  const timers = new ManualTimers(), c = new ProbeController(fakeCanvas(), { WorkerClass: FakeWorker, timers, maxStarts: 2 });
  const old = await ready(c), current = await ready(c);
  assert.equal(old.worker.terminated, true); assert(current.generation > old.generation);
  const stale = fakeBitmap(); old.worker.emit({ type: 'frame', generation: old.generation, sequence: 1, bitmap: stale });
  assert.equal(stale.closed, 1); assert.equal(c.stats.presented, 0);
  await assert.rejects(c.start('tiny-pen'), /restart budget/); assert.equal(current.worker.terminated, true);
});
test('canvas presentation failure disposes bitmap and retires pending command', async () => {
  const c = new ProbeController({ getContext() { throw new Error('context lost'); } }, { WorkerClass: FakeWorker, timers: new ManualTimers() });
  const { worker, generation } = await ready(c); const promise = c.initial(), failure = assert.rejects(promise, /context lost/), bitmap = fakeBitmap();
  worker.emit({ type: 'frame', generation, sequence: 1, bitmap }); await failure;
  assert.equal(bitmap.closed, 1); assert.equal(worker.terminated, true);
});

test('DOM metadata sinks fail closed; stand-in class tests NO native canvas operations', () => {
  class MetadataOnlyCanvas { #w = 1; #h = 1; get width() { return this.#w; } set width(v) { this.#w = v; } get height() { return this.#h; } set height(v) { this.#h = v; } getContext() { throw new Error('Node canvas operations prohibited'); } }
  const scope = { OffscreenCanvas: MetadataOnlyCanvas, ImageData: class {} }, boundary = installWorkerBoundary(scope), doc = scope.document;
  assert.throws(() => doc.createElement('div'), /boundary/); assert.throws(() => doc.getElementById('unknown'), /boundary/);
  assert.throws(() => doc.querySelector, /boundary/); assert.throws(() => doc.documentElement.clientWidth, /boundary/);
  const canvas = doc.createElement('canvas'); canvas.id = 'tegaki-canvas'; canvas.width = 24; canvas.height = 12;
  assert(canvas instanceof MetadataOnlyCanvas); assert.equal(canvas.width, 24);
  assert.throws(() => canvas.width = 25, /geometry/); assert.throws(() => canvas.style.transform = '', /boundary/);
  assert.throws(() => canvas.cloneNode, /boundary/); assert.throws(() => canvas.getContext('2d'), /prohibited/);
  boundary.containers.layers.appendChild(canvas);
  const layer = doc.createElement('canvas'); layer.id = 'tegaki-canvas-1'; layer.className = 'tegaki-layer'; layer.setAttribute('data-id', 1);
  boundary.containers.layers.insertBefore(layer, canvas.nextElementSibling);
  assert.equal(canvas.nextElementSibling, layer); assert.equal(layer.parentNode, boundary.containers.layers);
  assert.deepEqual(boundary.containers.layers.getElementsByClassName('tegaki-layer'), [layer]);
});
test('static fixture is owned and read-only without binding a port', async () => {
  const files = await staticFiles(), handler = makeHandler(files);
  for (const [url, method, status] of [['/', 'GET', 200], ['/worker.mjs', 'HEAD', 200], ['/generated/tegaki-icons.v1.woff', 'GET', 200], ['/../../package.json', 'GET', 404], ['/index.html', 'POST', 404]]) {
    const response = { writeHead(value) { this.status = value; return this; }, end() {} };
    handler({ url, method }, response); assert.equal(response.status, status);
  }
});

// Fixture sanity only. The actual Rust check_core_v1 test remains a separate
// qualification lane; this small transcript check does not replace that model.
test('fixed corpus has paired in-canvas strokes and valid creation-only layer/history references', () => {
  for (const item of CASES) {
    const layers = [{ id: 1, visible: true }], selected = new Set([1]);
    let active = 1, nextId = 1, stroke = false, undo = [], redo = [];
    const push = action => {
      if (action.kind === 'alpha' && undo.at(-1)?.kind === 'alpha'
          && undo.at(-1).selected.join() === action.selected.join()) return;
      undo.push(action); if (undo.length > 50) undo.shift(); redo = [];
    };
    for (const [tag, ...args] of item.events) {
      if (stroke) assert([2, 8, 3].includes(tag), `${item.id}: setting inside stroke`);
      if ([1, 2, 7, 8].includes(tag)) {
        assert(args[0] >= 0 && args[0] < item.width && args[1] >= 0 && args[1] < item.height, item.id);
        if ([1, 7].includes(tag)) { assert(!stroke); assert(layers.find(layer => layer.id === active).visible, item.id); stroke = true; }
        else assert(stroke, item.id);
      } else if (tag === 3) { assert(stroke, item.id); stroke = false; push({ kind: 'draw', layer: active }); }
      else if (tag === 20) {
        assert(layers.length < 8); const index = layers.findIndex(layer => layer.id === active);
        active = ++nextId; layers.splice(index + 1, 0, { id: active, visible: true }); selected.clear(); selected.add(active); push({ kind: 'add' });
      } else if (tag === 24) {
        const layer = layers.find(layer => layer.id === args[0]); assert(layer, `${item.id}: visibility ID`); layer.visible = !layer.visible;
      } else if (tag === 25) {
        active = args[0] || layers.at(-1).id; assert(layers.some(layer => layer.id === active), item.id); selected.clear(); selected.add(active);
      } else if (tag === 26) {
        assert(layers.some(layer => layer.id === args[0]), item.id);
        if (selected.has(args[0])) selected.delete(args[0]); else selected.add(args[0]);
      } else if (tag === 27) { assert(selected.size > 0); push({ kind: 'alpha', selected: [...selected] }); }
      else if (tag === 254) push({ kind: 'dummy' });
      else if (tag === 4 || tag === 5) {
        const from = tag === 4 ? undo : redo, to = tag === 4 ? redo : undo, action = from.pop();
        assert(action && action.kind !== 'add', `${item.id}: invalid history boundary`);
        to.push(action);
        if (action.kind === 'draw') { active = action.layer; selected.clear(); selected.add(active); }
      }
    }
    assert(!stroke, item.id);
  }
});

test('synchronous command and acknowledgement send errors retire pending work', async () => {
  for (const phase of ['command', 'ack']) {
    const c = new ProbeController(fakeCanvas(), { WorkerClass: FakeWorker, timers: new ManualTimers() });
    const { worker, generation } = await ready(c);
    if (phase === 'ack') {
      const promise = c.initial(); worker.emit({ type: 'frame', generation, sequence: 1, bitmap: fakeBitmap() }); await promise;
    }
    worker.postMessage = () => { throw new Error('send failed'); };
    await assert.rejects(phase === 'command' ? c.initial() : c.acknowledge(), /send failed/);
    assert.equal(worker.terminated, true); assert.equal(c.entry, null);
  }
});
test('fault-entry diagnostics prove observation without renewing parent deadline', async () => {
  const timers = new ManualTimers(), c = new ProbeController(fakeCanvas(), { WorkerClass: FakeWorker, timers, fault: 'startup' });
  const promise = c.start('pencil-pressure'), failure = assert.rejects(promise, /deadline/);
  const worker = FakeWorker.instances.at(-1), generation = worker.messages[0].generation, timer = timers.next;
  worker.emit({ type: 'stall-entered', generation }); assert.equal(c.stats.stallEntries, 1); assert.equal(timers.next, timer);
  timers.fire(); await failure; assert.equal(worker.terminated, true);
});

test('each comparison channel rejects its own one-byte mutation, preserving other channels', async () => {
  const { COMPARISON_CHANNELS, comparisonNegative, compareObservation } = await import('../fixtures/replay-worker/comparison.mjs');
  const expected = { eventIndex: 2, layers: [{ authoritative: [4, 5, 6, 255], nativeReadback: [4, 5, 6, 255] }] };
  const flattened = [4, 5, 6, 255], actual = { state: structuredClone(expected), flattened: [...flattened], presented: [...flattened] };
  for (const channel of COMPARISON_CHANNELS) assert.equal(comparisonNegative(actual, expected, flattened, 2, channel).channel, channel);
  compareObservation(actual, expected, flattened, 2);
  // Synthetic Node observations test assertion wiring only, not native pixels.
});
test('independent semantic checkpoint detects a shared wrong pixel and shared wrong pressure', async () => {
  const { checkSemanticCheckpoint } = await import('../fixtures/replay-worker/semantic-checkpoints.mjs');
  const bytes = new Array(24 * 24 * 4).fill(0), flattened = Array.from({ length: 24 * 24 * 4 }, (_, i) => [240, 246, 251, 255][i % 4]);
  bytes.splice(4 * (4 * 24 + 3), 4, 43, 61, 79, 255); flattened.splice(4 * (4 * 24 + 3), 4, 43, 61, 79, 255);
  const state = { eventIndex: 2, dimensions: [24, 24], layers: [{ id: 1, authoritative: bytes, nativeReadback: [...bytes] }],
    pressure: [49151 / 65535, 49151 / 65535], pending: { kind: 'Draw', layerId: 1, imageDataBefore: { data: new Array(bytes.length).fill(0) } }, undo: [], isPainting: false };
  assert(checkSemanticCheckpoint('pencil-pressure', state, flattened, 'synthetic') > 0);
  const wrongPixel = structuredClone(state); wrongPixel.layers[0].authoritative[4 * (4 * 24 + 3)] = 0;
  assert.throws(() => checkSemanticCheckpoint('pencil-pressure', wrongPixel, flattened, 'synthetic'), /literal pencil start authoritative/);
  const wrongPressure = structuredClone(state); wrongPressure.pressure = [0.5, 0.5];
  assert.throws(() => checkSemanticCheckpoint('pencil-pressure', wrongPressure, flattened, 'synthetic'), /encoded start pressure/);
});
test('direct native-worker probe bypasses controller and closes every received bitmap', async () => {
  const source = await readFile(new URL('../fixtures/replay-worker/raw-worker-protocol.mjs', import.meta.url), 'utf8');
  assert.match(source, /new Worker\(new URL\('\.\/worker\.mjs'/);
  assert.doesNotMatch(source, /^import .*controller/m);
  assert.match(source, /finally \{ bitmap\.close\(\)/);
  for (const name of ['step-without-ack', 'wrong-ack-sequence', 'duplicate-ack', 'skipped-command-sequence']) assert(source.includes(name));
});
