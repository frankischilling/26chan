import test from 'node:test';
import assert from 'node:assert/strict';
import { DRAWING_LIMITS, drawingDimensions, drawingPngFile, createDrawingUpload } from '../../apps/public/client/native-drawing-core.js';
import { drawingPostingResult, postingResult, parseQuickReplyUpload } from '../../apps/public/client/native-quick-reply-transport.js';

const receipt = (id = 'a', resto = '41', state = 'queued') => ({ upload_id: id.repeat(32), upload_capability: id.repeat(64), resto, state });
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
function harness(overrides = {}) {
  const calls = [], changes = [], timers = new Map(); let target = '41', counter = 0;
  const value = createDrawingUpload({ board: 'qst', target: () => target, changed: state => changes.push(state),
    upload: async args => { calls.push(['upload', args]); return receipt(); },
    check: async args => { calls.push(['check', args]); return { ...args.receipt, state: 'approved' }; },
    cancel: async args => { calls.push(['cancel', args]); },
    schedule: (callback, delay) => { timers.set(++counter, { callback, delay }); return counter; },
    unschedule: id => timers.delete(id), ...overrides });
  return { value, calls, changes, timers, target(next) { target = next; } };
}

test('drawing dimensions distinguish decoder safety bounds from unused source i100–800 constants', () => {
  assert.deepEqual(DRAWING_LIMITS, { side: 1024, pixels: 1048576, bytes: 8388608 });
  for (const value of [1, '1', 99, 100, 400, 800, 801, 1024, '1024']) {
    assert.deepEqual(drawingDimensions(value, value), { width: Number(value), height: Number(value) });
  }
  for (const value of [0, -1, 1025, Infinity, NaN, null, true, {}, '01', '1.5', '1e2', ' 400', '+1', '1024\n']) {
    assert.throws(() => drawingDimensions(value, 400)); assert.throws(() => drawingDimensions(400, value));
  }
});

test('PNG export rejects mismatched headers, empty blobs and export exceptions before transport', async () => {
  const header = Buffer.alloc(33); Buffer.from([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]).copy(header);
  header.writeUInt32BE(400, 16); header.writeUInt32BE(300, 20);
  // Header witnesses only. Real decoded pixels are checked in drawing-upload.mjs.
  const canvas = { width: 400, height: 300, toBlob: callback => callback(new Blob([header], { type: 'image/png' })) };
  const file = await drawingPngFile(canvas); assert.equal(file.name, 'tegaki.png'); assert.equal(file.type, 'image/png');
  await assert.rejects(drawingPngFile({ ...canvas, width: 401 }), /invalid PNG/);
  await assert.rejects(drawingPngFile({ ...canvas, toBlob: callback => callback(null) }), /export failed/);
  await assert.rejects(drawingPngFile({ ...canvas, toBlob: callback => callback(new Blob([])) }), /PNG upload bounds/);
  await assert.rejects(drawingPngFile({ ...canvas, toBlob: () => { throw new Error('tainted'); } }), /tainted/);
  await assert.rejects(drawingPngFile({ ...canvas, width: 1025 }), /dimensions/);
});

test('ordinary new-thread upload target is exact zero, while normal QR still rejects zero', () => {
  const data = JSON.stringify(receipt('a', '0'));
  assert.equal(parseQuickReplyUpload(data, '0').resto, '0');
  assert.deepEqual(drawingPostingResult('{"tid":0,"pid":9007199254740993}', '0'), { thread: '9007199254740993', post: '9007199254740993' });
  assert.throws(() => postingResult('{"tid":0,"pid":42}', '0'));
  for (const text of ['{"tid":1,"pid":42}', '{"tid":0,"pid":0}', '{"tid":0,"pid":42,"url":"//evil"}', '{"tid":"0","pid":42}']) assert.throws(() => drawingPostingResult(text, '0'));
});

test('upload lifecycle exposes no approval until exact upload receipt is approved by status', async () => {
  const h = harness(); assert.equal(h.value.snapshot().phase, 'empty');
  await h.value.select(new Blob(['drawing']));
  assert.equal(h.value.snapshot().approved, false); assert.equal(h.value.snapshot().phase, 'queued');
  assert.deepEqual([...h.timers.values()].map(value => value.delay), [1000]);
  await h.value.status();
  assert.equal(h.value.snapshot().approved, true); assert.equal(h.value.snapshot().receipt.state, 'approved');
  assert.equal(h.calls[1][1].receipt.upload_id, 'a'.repeat(32)); h.value.dispose();
});

test('late upload after Clear cannot attach, and its actual owned receipt is canceled', async () => {
  const pending = deferred(); const h = harness({ upload: () => pending.promise });
  const select = h.value.select(new Blob(['drawing'])); await tick();
  assert.equal(h.value.snapshot().phase, 'uploading'); await h.value.clear();
  pending.resolve(receipt()); assert.equal(await select, false); await tick();
  assert.equal(h.value.snapshot().phase, 'empty'); assert.equal(h.value.snapshot().receipt, null);
  const cleanup = h.calls.find(value => value[0] === 'cancel')[1];
  assert.deepEqual(cleanup.receipt, receipt()); assert.equal(cleanup.thread, '41'); assert.equal(cleanup.keepalive, true);
});

test('file/drawing replacement cancels previous receipt before starting its replacement', async () => {
  const h = harness(); await h.value.select(new Blob(['first']));
  await h.value.select(new Blob(['replacement']));
  assert.deepEqual(h.calls.map(value => value[0]), ['upload', 'cancel', 'upload']); h.value.dispose();
});

test('failed cancellation retains approval and blocks a replacement upload', async () => {
  const h = harness({ cancel: async () => { throw new Error('retry cancel'); } });
  await h.value.select(new Blob(['drawing'])); await h.value.status();
  assert.equal(await h.value.select(new Blob(['file'])), false);
  assert.equal(h.calls.filter(value => value[0] === 'upload').length, 1);
  assert.equal(h.value.snapshot().approved, true); assert.equal(h.value.snapshot().error, 'retry cancel'); h.value.dispose();
});

test('late status cannot restore a cleared attachment', async () => {
  const pending = deferred(); const h = harness({ check: () => pending.promise });
  await h.value.select(new Blob(['drawing'])); const check = h.value.status();
  await h.value.clear(); pending.resolve(receipt('a', '41', 'approved'));
  assert.equal(await check, false); assert.equal(h.value.snapshot().receipt, null); assert.equal(h.value.snapshot().phase, 'empty');
});

test('QR target change cancels old target and rejects old upload or status completion', async () => {
  for (const stage of ['upload', 'check']) {
    const pending = deferred(); const h = harness({ [stage]: () => pending.promise });
    let work;
    if (stage === 'upload') { work = h.value.select(new Blob(['drawing'])); await tick(); }
    else { await h.value.select(new Blob(['drawing'])); work = h.value.status(); }
    h.target('42'); h.value.resetTarget(); pending.resolve(receipt('a', '41', 'approved')); assert.equal(await work, false); await tick();
    assert.equal(h.value.snapshot().receipt, null); assert.equal(h.value.snapshot().approved, false);
    assert.equal(h.calls.filter(value => value[0] === 'cancel').at(-1)[1].thread, '41'); h.value.dispose();
  }
});

test('pagehide/disposal aborts work and cleans its owned receipt without emitting stale UI changes', async () => {
  const pending = deferred(); let signal; const h = harness({ upload: args => { signal = args.signal; return pending.promise; } });
  const work = h.value.select(new Blob(['drawing'])); await tick(); h.value.dispose(); const changes = h.changes.length;
  assert.equal(signal.aborted, true); pending.resolve(receipt()); assert.equal(await work, false); await tick();
  assert.equal(h.changes.length, changes); assert.equal(h.value.snapshot().receipt, null);
  assert.equal(await h.value.select(new Blob(['new'])), false);
});

test('posting owns approved capability and blocks Clear/replacement/status; consumed state is retired', async () => {
  const h = harness(); await h.value.select(new Blob(['drawing'])); await h.value.status();
  h.value.posting(true); const count = h.calls.length;
  assert.equal(await h.value.clear(), false); assert.equal(await h.value.select(new Blob(['new'])), false);
  assert.equal(await h.value.status(), false); assert.equal(h.calls.length, count);
  h.value.retire(); assert.equal(h.value.snapshot().receipt, null); assert.equal(h.value.snapshot().phase, 'empty');
  h.value.dispose(); assert.equal(h.calls.length, count);
});

test('automatic status retry is bounded at source-independent transport limits and manual check remains', async () => {
  const h = harness({ check: async args => args.receipt }); await h.value.select(new Blob(['drawing']));
  const delays = [];
  while (h.timers.size) {
    const [key, task] = [...h.timers][0]; h.timers.delete(key); delays.push(task.delay); task.callback(); await tick();
  }
  assert.deepEqual(delays, [1000, 2000, 4000]); assert.equal(h.value.snapshot().canCheck, true);
  assert.equal(await h.value.status(), true); assert.equal(h.timers.size, 0); h.value.dispose();
});
