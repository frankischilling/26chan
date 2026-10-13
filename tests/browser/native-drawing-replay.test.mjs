import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { drawingReplayFile, DRAWING_REPLAY_LIMITS } from '../../apps/public/client/native-drawing-replay.js';
import { createDrawingPainter } from '../../apps/public/client/native-drawing-painter.js';

function setup() {
  const state = { requested: true, files: [], errors: [], clears: 0, asks: 0 };
  const painter = { bg: null, baseWidth: 16, baseHeight: 16, saveReplay: false,
    open(options) { if (this.bg) return; this.bg = {}; this.saveReplay = options.saveReplay; },
    replayRecorder: { toBlob: () => new Blob(['recorded']) },
    flatten: () => ({}), destroy() { this.bg = null; }, hide() {},
  };
  const client = { key: 'ordinary', target: '0', disposed: () => false,
    recordReplay: () => state.requested, prepare: async () => true,
    loading() {}, exporting() {}, replaced() {}, cancelled() {},
    clear: async () => true, clearForReplacement: async () => { state.clears++; return true; },
    error: value => state.errors.push(value), finished: (...values) => state.files.push(values),
  };
  const options = { load: async () => painter, exportFile: async () => 'png',
    exportReplay: async () => 'replay', confirmReplacement: () => { state.asks++; return true; } };
  return { state, painter, client, options };
}

test('actual pinned recorder output remains byte-exact and uses a fixed upload name', async () => {
  for (const name of ['empty', 'commands']) {
    const bytes = await readFile(new URL(`../../apps/media-guest/tests/fixtures/replay/${name}.tgkr`, import.meta.url));
    const file = await drawingReplayFile(new Blob([bytes], { type: 'text/html' }));
    assert.equal(file.name, 'tegaki.tgkr');
    assert.equal(file.type, 'application/octet-stream');
    assert.deepEqual(new Uint8Array(await file.arrayBuffer()), new Uint8Array(bytes));
  }
});

test('header hint rejects empty, oversized and wrong-magic input without decoding', async () => {
  const bytes = new Uint8Array(13);
  bytes.set([0x54, 0x47, 0x4b, 1]);
  new DataView(bytes.buffer).setUint32(4, 1);
  assert.equal((await drawingReplayFile(new Blob([bytes]))).size, 13);
  for (const bad of [null, {}, new Blob([]), new Blob([bytes.slice(0, 12)]),
    new Blob([new Uint8Array(DRAWING_REPLAY_LIMITS.bytes + 1)])]) {
    await assert.rejects(drawingReplayFile(bad));
  }
  for (let index = 0; index < 4; index++) {
    const bad = bytes.slice(); bad[index] ^= 1;
    await assert.rejects(drawingReplayFile(new Blob([bad])));
  }
  for (const size of [0, DRAWING_REPLAY_LIMITS.inflatedBytes + 1, 0xffffffff]) {
    new DataView(bytes.buffer).setUint32(4, size);
    await assert.rejects(drawingReplayFile(new Blob([bytes])));
  }
  new DataView(bytes.buffer).setUint32(4, DRAWING_REPLAY_LIMITS.inflatedBytes);
  assert.equal((await drawingReplayFile(new Blob([bytes]))).size, 13);
});

test('a plausible header is deliberately not a replay-validation claim', async () => {
  const bytes = new Uint8Array(13);
  bytes.set([0x54, 0x47, 0x4b, 1]);
  new DataView(bytes.buffer).setUint32(4, 123);
  bytes[12] = 0xff; // Invalid compressed data is left to the isolated decoder.
  assert.equal((await drawingReplayFile(new Blob([bytes]))).size, 13);
});

test('Finish transfers the PNG and recorded replay together, never a replay-only result', async () => {
  const f = setup(), controller = createDrawingPainter(f.options);
  assert.equal(await controller.open(f.client, 16, 16), true);
  assert.equal(f.painter.saveReplay, true);
  await f.painter.onDoneCb();
  assert.deepEqual(f.state.files, [['png', 0, 'replay']]);
  controller.dispose();
});

test('changing recording mode replaces retained layers only after consent and cancellation', async () => {
  const f = setup(), controller = createDrawingPainter(f.options);
  await controller.open(f.client, 16, 16); await f.painter.onDoneCb();
  f.state.requested = false;
  await controller.open(f.client, 16, 16);
  assert.equal(f.state.asks, 1); assert.equal(f.state.clears, 1);
  assert.equal(f.painter.saveReplay, false);
  await f.painter.onDoneCb();
  assert.deepEqual(f.state.files.at(-1), ['png', 0, null]);
  controller.dispose();
});

test('recording failure does not silently upload only the raster', async () => {
  const f = setup(); f.options.exportReplay = async () => { throw new Error('invalid replay'); };
  const controller = createDrawingPainter(f.options);
  await controller.open(f.client, 16, 16); await f.painter.onDoneCb();
  assert.equal(f.state.files.length, 0); assert.equal(f.state.errors.length, 1);
  controller.dispose();
});

test('late replay export is fenced after the owner is cleared', async () => {
  const f = setup(); let resolve;
  f.options.exportReplay = () => new Promise(done => { resolve = done; });
  const controller = createDrawingPainter(f.options);
  await controller.open(f.client, 16, 16);
  const done = f.painter.onDoneCb();
  await new Promise(next => setImmediate(next));
  controller.invalidate(f.client); resolve('replay'); await done;
  assert.equal(f.state.files.length, 0);
  controller.dispose();
});

import { createDrawingUpload } from '../../apps/public/client/native-drawing-core.js';
const uploadReceipt = { resto: '0', state: 'approved', upload_id: '1'.repeat(32), upload_capability: '2'.repeat(64) };
test('recorded drawing uses exactly one paired upload and one receipt', async () => {
  const calls = [], png = new Blob(['png']), replay = new Blob(['replay']);
  const transfer = createDrawingUpload({ board: 'i', target: () => '0',
    upload: async () => { throw new Error('image-only downgrade'); },
    uploadPair: async args => { calls.push(args); return uploadReceipt; }, cancel: async () => {},
  });
  assert.equal(await transfer.select(png, replay), true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].file, png); assert.equal(calls[0].replay, replay);
  assert.equal(transfer.snapshot().receipt, uploadReceipt);
  transfer.dispose();
});

test('missing paired transport keeps the existing receipt and does not upload or cancel', async () => {
  let uploads = 0, cancels = 0;
  const transfer = createDrawingUpload({ board: 'i', target: () => '0',
    upload: async () => { uploads++; return uploadReceipt; }, cancel: async () => { cancels++; },
  });
  assert.equal(await transfer.select(new Blob(['png'])), true);
  assert.equal(await transfer.select(new Blob(['new png']), new Blob(['replay'])), false);
  assert.equal(uploads, 1); assert.equal(cancels, 0);
  assert.equal(transfer.snapshot().receipt, uploadReceipt);
  assert.match(transfer.snapshot().error, /Replay upload is unavailable/);
  transfer.dispose();
});

test('stale paired completion retires its receipt instead of reattaching it', async () => {
  let complete, started;
  const entered = new Promise(resolve => { started = resolve; });
  const canceled = [];
  const transfer = createDrawingUpload({ board: 'i', target: () => '0',
    uploadPair: args => { started(args); return new Promise(resolve => { complete = resolve; }); },
    cancel: async args => { canceled.push(args.receipt); },
  });
  const pending = transfer.select(new Blob(['png']), new Blob(['replay']));
  const args = await entered;
  transfer.resetTarget(); assert.equal(args.signal.aborted, true);
  complete(uploadReceipt);
  assert.equal(await pending, false);
  assert.equal(transfer.snapshot().receipt, null);
  assert.deepEqual(canceled, [uploadReceipt]);
  transfer.dispose();
});


test('drawing transport sends the exact ordered atomic pair and never retries as PNG-only', async () => {
  const { uploadDrawingFile } = await import('../../apps/public/client/native-quick-reply-transport.js');
  const file = new File([new Uint8Array(33)], 'tegaki.png', { type: 'image/png' });
  const recording = new File([new Uint8Array(13)], 'tegaki.tgkr');
  const receipt = { upload_id: 'a'.repeat(32), upload_capability: 'b'.repeat(64), resto: '0', state: 'queued' };
  let calls = 0;
  for (const replay of [null, recording]) {
    const result = await uploadDrawingFile({ origin: 'https://board.example', board: 'i', thread: '0', file, replay,
      fetcher: async (url, options) => {
        calls++;
        assert.equal(url, 'https://board.example/i/drawing');
        assert.equal(options.credentials, 'same-origin'); assert.equal(options.redirect, 'error');
        assert.deepEqual([...options.body.keys()], ['resto', 'png_bytes', 'replay_bytes', 'upfile', ...(replay ? ['replay'] : [])]);
        assert.equal(options.body.get('png_bytes'), '33'); assert.equal(options.body.get('replay_bytes'), replay ? '13' : '0');
        assert.equal(options.body.get('upfile').name, 'tegaki.png');
        return new Response(JSON.stringify(receipt), { headers: { 'content-type': 'application/json' } });
      } });
    assert.deepEqual(result, receipt);
  }
  assert.equal(calls, 2);
  await assert.rejects(uploadDrawingFile({ origin: 'https://board.example', board: 'i', thread: '0', file, replay: recording,
    fetcher: async () => { calls++; throw new Error('isolated intake unavailable'); } }), /Keep the drawing/);
  assert.equal(calls, 3);
  for (const replay of [{}, new File([], 'tegaki.tgkr'), new File([new Uint8Array(13)], 'other.tgkr')]) {
    await assert.rejects(uploadDrawingFile({ origin: 'https://board.example', board: 'i', thread: '0', file, replay,
      fetcher: async () => { calls++; throw new Error('must not send'); } }), /Invalid drawing recording/);
  }
  assert.equal(calls, 3);
});
