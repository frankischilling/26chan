import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { decodeReplayCandidateWire as decode } from '../../apps/public/client/native-replay-data.js';
import { prepareBoundedReplayCandidate as prepare } from '../../apps/public/client/native-replay-envelope.js';
import { createInertReplayRuntime } from '../helpers/pinned-replay-runtime.mjs';
const bytes = await readFile(new URL('../media/fixtures/replay-wire/empty.ibr', import.meta.url));
function changed(offset, change) {
  const out = Uint8Array.from(bytes);
  change(new DataView(out.buffer), offset); return decode(out);
}

test('numeric envelope preserves real recorder defaults and does not execute events', async () => {
  const runtime = await createInertReplayRuntime();
  assert.equal(prepare(decode(bytes), runtime.viewer).events.length, 2);
  assert.deepEqual(runtime.calls, []);
});
test('invalid numeric data is rejected before any source constructor provider call', () => {
  let calls = 0;
  const provider = { getEventIdMap() { calls++; throw new Error('must not reach provider'); } };
  for (const value of [
    changed(65, (view, offset) => view.setUint8(offset, 0)),
    changed(65, (view, offset) => view.setUint8(offset, 65)),
    changed(68, (view, offset) => view.setFloat32(offset, 1.01)),
    changed(68, (view, offset) => view.setFloat32(offset, -0.01)),
    changed(72, (view, offset) => view.setFloat32(offset, .02)),
    changed(67, (view, offset) => view.setInt8(offset, -1)),
    changed(40, (view, offset) => view.setUint32(offset, 0)),
  ]) assert.throws(() => prepare(value, provider), error => error.code === 'numeric-envelope');
  assert.equal(calls, 0);
});
test('forged candidates cannot invoke getters or providers during numeric preflight', () => {
  let calls = 0;
  const value = { get metadata() { calls++; throw new Error('getter'); } };
  assert.throws(() => prepare(value, { getEventIdMap() { calls++; } }), error => error.code === 'candidate-brand');
  assert.equal(calls, 0);
});

function withEvents(records) {
  const out = new Uint8Array(256 + 16 * (records.length + 2));
  out.set(bytes.subarray(0, 256));
  const view = new DataView(out.buffer);
  view.setUint32(16, out.length); view.setUint32(20, records.length + 2);
  out[256] = 0;
  records.forEach(([tag, value, time = 0], index) => {
    const offset = 272 + 16 * index; out[offset] = tag; view.setUint32(offset + 4, time);
    if ([12,18,27].includes(tag)) view.setFloat32(offset + 8, value);
    else if ([1,2,7,8].includes(tag)) { view.setInt16(offset + 8, value[0]); view.setInt16(offset + 10, value[1]); }
    else if (value !== undefined) out[offset + 8] = value;
  });
  const end = out.length - 16; out[end] = 255;
  view.setUint32(end + 4, records.at(-1)?.[2] ?? 0);
  return decode(out);
}
const inertProvider = { getEventIdMap: () => Object.fromEntries(
  [0,1,2,3,4,5,6,7,8,10,11,12,13,14,15,16,17,18,20,21,22,23,24,25,26,27,254,255].map(tag => [tag, class {}])) };
test('full vocabulary and enabled dynamics remain available inside the numeric envelope', () => {
  const records = [[13,1],[14,1],[17,1],[20],[21],[22,1],[23],[4],[5],[254]];
  assert.equal(prepare(withEvents(records), inertProvider).events.length, records.length + 2);
});
test('numeric event settings and chronology are checked before constructors', () => {
  for (const records of [
    [[11,0]], [[11,65]], [[12,-.1]], [[18,1.01]], [[27,-.01]],
    [[15,1]], [[10,8],[15,3]], [[20,undefined,20],[4,undefined,19]],
    [[20,undefined,86400001]], [[7,[-32768,0]]], [[7,[0,32767]]],
  ]) assert.throws(() => prepare(withEvents(records), inertProvider), error => error.code === 'numeric-envelope');
  assert.doesNotThrow(() => prepare(withEvents([[10,8],[15,2],[7,[-1,0]]]), inertProvider));
});
