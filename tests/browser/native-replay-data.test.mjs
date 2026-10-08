import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import {
  decodeReplayCandidateWire as decode, prepareReplayCandidate as prepare,
  REPLAY_WIRE_LIMITS, ReplayCandidateError,
} from '../../apps/public/client/native-replay-data.js';
import { createInertReplayRuntime, ownEventFields } from '../helpers/pinned-replay-runtime.mjs';

const oracle = JSON.parse(await readFile(new URL('../fixtures/native-replay-source.json', import.meta.url)));
const emptyWire = await readFile(new URL('../media/fixtures/replay-wire/empty.ibr', import.meta.url));
const commandsWire = await readFile(new URL('../media/fixtures/replay-wire/commands.ibr', import.meta.url));
const tags = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27, 254, 255];
const unsupported = [13, 14, 17, 21, 22, 23];

// Literal independent framing; no encoder or production decoder makes it.
// Reversed epochs/timestamps, out-of-range tool size/tip/step are intentional.
function literal() {
  const header = [
    73, 66, 82, 80, 76, 89, 48, 49, 0, 1, 0, 64, 0, 1, 0, 0,
    0, 0, 1, 32, 0, 0, 0, 2, 2, 128, 1, 224, 255, 128, 0, 12, 34, 56,
    8, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 9, 4, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
  ];
  const tool = [1, 255, 15, 128, 63, 0, 0, 0, 194, 200, 0, 0, 63, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const prelude = [0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0];
  const conclusion = [255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  return Uint8Array.from([...header, ...Array.from({ length: 8 }, (_, i) => [i + 1, ...tool.slice(1)]).flat(), ...prelude, ...conclusion]);
}
const view = bytes => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
function framed(records, base = literal()) {
  const bytes = new Uint8Array(256 + records.length * 16);
  bytes.set(base.subarray(0, 256));
  records.forEach((record, i) => bytes.set(record, 256 + 16 * i));
  view(bytes).setUint32(16, bytes.length); view(bytes).setUint32(20, records.length);
  return bytes;
}
function record(tag, payload = [], time = 0x12345678) {
  const bytes = new Uint8Array(16);
  bytes[0] = tag; bytes.set(payload, 8); view(bytes).setUint32(4, time);
  return bytes;
}
function withEvent(tag, payload = [], time) {
  return framed([record(0), record(tag, payload, time), record(255)]);
}
function fails(bytes, code) {
  assert.throws(() => decode(bytes), error => error instanceof ReplayCandidateError && error.code === code);
}
function changed(bytes, offset, value) { const copy = Uint8Array.from(bytes); copy[offset] = value; return copy; }
function commandsCore() {
  const records = [];
  for (let offset = 256; offset < commandsWire.length; offset += 16) {
    if (!unsupported.includes(commandsWire[offset])) records.push(commandsWire.subarray(offset, offset + 16));
  }
  return framed(records, commandsWire);
}

test('independent recorder fixtures and actual source constructors stay pinned', async () => {
  assert.equal(commandsWire.length, oracle.wire.bytes);
  assert.equal(createHash('sha256').update(commandsWire).digest('hex'), oracle.wire.sha256);
  assert.equal(createHash('sha256').update(emptyWire).digest('hex'), 'c857e7332d4ab078639052ac1228e19e8935c18f72fbc1d50b8fe8aba05ac4cc');
  const runtime = await createInertReplayRuntime();
  assert.deepEqual(runtime.source, oracle.source);
  const constructors = runtime.viewer.getEventIdMap();
  assert.deepEqual(Object.keys(constructors).map(Number), tags);
  for (const row of oracle.events) {
    const event = new constructors[row.tag](...row.arguments);
    assert.equal(event.constructor.name, row.constructor);
    assert.deepEqual(ownEventFields(event), row.ownFields);
  }
  assert.deepEqual(runtime.calls, []);
});

test('decoder preserves all 28 tags and independently specified source command fields', () => {
  const candidate = decode(commandsWire);
  assert.deepEqual([...new Set(candidate.events.map(event => event.tag))].sort((a, b) => a - b), tags);
  assert.equal(candidate.events.length, oracle.events.length);
  candidate.events.forEach((event, index) => {
    const expected = oracle.events[index];
    assert.equal(event.tag, expected.tag);
    assert.equal(event.timestampMs, expected.arguments[0]);
    if ([1, 2].includes(event.tag)) assert.deepEqual([event.x, event.y, event.pressure], expected.arguments.slice(1));
    else if ([7, 8].includes(event.tag)) {
      assert.deepEqual([event.x, event.y], expected.arguments.slice(1));
      assert.equal(Object.hasOwn(event, 'pressure'), false);
    } else if (event.tag === 6) assert.deepEqual(event.color, expected.arguments[1]);
    else if (expected.arguments.length === 2) assert.equal(event.value, expected.arguments[1]);
    else assert.equal(Object.hasOwn(event, 'value'), false);
  });
});

test('preparation uses real source constructors, exact own fields and metadata/toolMap shapes', async () => {
  const runtime = await createInertReplayRuntime();
  const before = { ...runtime.viewer };
  const candidate = decode(commandsCore()), result = prepare(candidate, runtime.viewer);
  assert.equal(result.kind, 'untrusted-tegaki-preparation');
  assert.equal(result.candidateProfile, 1);
  assert.deepEqual(result.metadata, { canvasWidth: 640, canvasHeight: 480,
    startTimeStamp: 1690000013000, endTimeStamp: 1690000025000,
    bgColor: [255, 255, 255], toolColor: [0, 0, 0], toolId: 1 });
  const expectedRows = oracle.events.filter(row => !unsupported.includes(row.tag));
  assert.equal(result.events.length, expectedRows.length);
  const constructors = runtime.viewer.getEventIdMap();
  result.events.forEach((event, i) => {
    assert.equal(event.constructor, constructors[expectedRows[i].tag]);
    assert.deepEqual(ownEventFields(event), expectedRows[i].ownFields);
    assert.equal(Object.isFrozen(event), true);
  });
  const defaults = [
    [1, 1, 1, 0.009999999776482582, 1], [2, 8, 1, 0.05000000074505806, 1],
    [3, 32, 1, 0.10000000149011612, 1], [4, 1, 1, 100, 0],
    [5, 8, 0.5, 0.009999999776482582, 1], [6, 1, 1, 100, 0],
    [7, 32, 0.5, 0.25, 0], [8, 8, 1, 0.10000000149011612, 0],
  ];
  assert.deepEqual(Object.keys(result.toolMap), ['1', '2', '3', '4', '5', '6', '7', '8']);
  for (const [id, size, alpha, step, usePreserveAlpha] of defaults) {
    assert.deepEqual(result.toolMap[id], { id, size, alpha, step, sizeDynamicsEnabled: 0,
      alphaDynamicsEnabled: 0, usePreserveAlpha, tipId: 0, flow: 1, flowDynamicsEnabled: 0 });
    assert.equal(Object.hasOwn(result.toolMap[id], 'preserveAlphaEnabled'), false);
  }
  assert.deepEqual({ ...runtime.viewer }, before, 'provider must not be hydrated/activated');
  for (const field of ['loaded', 'playing', 'duration', 'approved', 'authorized']) assert.equal(Object.hasOwn(result, field), false);
  assert.deepEqual(runtime.calls, []);
});

test('six non-core tags reject before consulting any constructor provider, even false dynamics', () => {
  for (const tag of unsupported) for (const value of [0, 1]) {
    const bytes = withEvent(tag, [21, 23].includes(tag) ? [] : [value]);
    const candidate = decode(bytes);
    assert.throws(() => prepare(candidate, new Proxy({}, { get() { assert.fail('provider consulted'); } })),
      { code: 'unsupported-core-tag' });
  }
  assert.throws(() => prepare(decode(commandsWire), null), { code: 'unsupported-core-tag' });
});

test('negative zero, finite float extremes and no-pressure constructor shapes remain exact', async () => {
  const { viewer, calls } = await createInertReplayRuntime();
  const bits = [0x80000000, 1, 0x80000001, 0x7f7fffff, 0xff7fffff, 0xbf800000, 0x3dcccccd];
  for (const bitsValue of bits) {
    const payload = new Uint8Array(4); view(payload).setUint32(0, bitsValue);
    const expected = view(payload).getFloat32(0);
    for (const tag of [12, 18, 27]) {
      const candidate = decode(withEvent(tag, payload));
      assert.ok(Object.is(candidate.events[1].value, expected));
      assert.ok(Object.is(prepare(candidate, viewer).events[1].value, expected));
    }
    const bytes = literal();
    for (let id = 0; id < 8; id++) for (const offset of [4, 8, 12]) view(bytes).setUint32(64 + id * 24 + offset, bitsValue);
    const candidate = decode(bytes), result = prepare(candidate, viewer);
    for (let id = 1; id <= 8; id++) for (const key of ['alpha', 'step', 'flow']) {
      assert.ok(Object.is(candidate.tools[id - 1][key], expected));
      assert.ok(Object.is(result.toolMap[id][key], expected));
    }
  }
  for (const tag of [1, 2, 7, 8]) {
    const candidate = decode(withEvent(tag, [128, 0, 127, 255, ...([1, 2].includes(tag) ? [0, 0] : [])], 0xffffffff));
    const event = prepare(candidate, viewer).events[1];
    assert.equal(event.x, -32768); assert.equal(event.y, 32767); assert.equal(event.timeStamp, 0xffffffff);
    assert.equal(Object.hasOwn(event, 'pressure'), tag === 1 || tag === 2);
    if (Object.hasOwn(event, 'pressure')) assert.equal(event.pressure, 0);
  }
  const result = prepare(decode(literal()), viewer);
  assert.equal(result.metadata.startTimeStamp, 4294967295000);
  assert.equal(result.metadata.endTimeStamp, 0);
  assert.equal(result.toolMap[1].tipId, -128);
  assert.equal(result.toolMap[1].step, -100);
  assert.equal(result.toolMap[1].size, 255);
  for (const field of ['sizeDynamicsEnabled', 'alphaDynamicsEnabled', 'flowDynamicsEnabled', 'usePreserveAlpha']) assert.equal(result.toolMap[1][field], 1);
  assert.deepEqual(calls, []);
});

test('bounded Uint8Array snapshot rejects arbitrary, shared, detached and resizable inputs', () => {
  for (const input of [null, {}, [], 'IBRPLY01', emptyWire.buffer, new DataView(emptyWire.buffer),
    new Uint16Array(144), new Uint8ClampedArray(288), new Proxy(literal(), {})]) fails(input, 'input-type');
  fails(new Uint8Array(new SharedArrayBuffer(288)), 'input-type');
  fails(new Uint8Array(new ArrayBuffer(288, { maxByteLength: 1024 })), 'input-type');
  const detached = literal(); structuredClone(detached.buffer, { transfer: [detached.buffer] });
  fails(detached, 'input-size');
  assert.equal(decode(Buffer.from(emptyWire)).events.length, 2);
  const foreign = vm.runInNewContext('Uint8Array.from(bytes)', { bytes: Array.from(emptyWire) });
  assert.equal(decode(foreign).events.length, 2);
  class HostileBytes extends Uint8Array {
    get byteLength() { assert.fail('caller byteLength getter'); }
    get buffer() { assert.fail('caller buffer getter'); }
    [Symbol.iterator]() { assert.fail('caller iterator'); }
    static get [Symbol.species]() { assert.fail('caller species'); }
  }
  const hostile = new HostileBytes(emptyWire);
  assert.equal(decode(hostile).events.length, 2);
  const enclosing = new Uint8Array(600); enclosing.set(emptyWire, 123);
  assert.equal(decode(enclosing.subarray(123, 123 + emptyWire.length)).events.length, 2);
});

test('candidate brand and frozen own data prevent forgery, mutation and post-decode byte changes', async () => {
  const { viewer, calls } = await createInertReplayRuntime();
  const input = commandsCore(), candidate = decode(input);
  const before = prepare(candidate, viewer);
  input.fill(255);
  const after = prepare(candidate, viewer);
  assert.deepEqual(after, before);
  for (const forged of [{}, { ...candidate }, structuredClone(candidate),
    new Proxy(candidate, {}), Object.create(candidate)]) {
    assert.throws(() => prepare(forged, null), { code: 'candidate-brand' });
  }
  for (const mutate of [
    () => { candidate.kind = 'approved'; }, () => { candidate.metadata.width = 0; },
    () => { candidate.metadata.background[0] = 0; }, () => { candidate.tools[0].alpha = NaN; },
    () => { candidate.events[1].color[0] = 256; }, () => { candidate.events[0].tag = 21; },
    () => { candidate.events.push({}); }, () => { candidate.tools.reverse(); },
    () => { before.metadata.canvasWidth = 0; }, () => { before.toolMap[1].alpha = Infinity; },
    () => { before.toolMap[9] = {}; }, () => { before.events[0].timeStamp = 5; },
    () => { before.events.push({}); },
  ]) assert.throws(mutate, TypeError);
  assert.deepEqual(calls, []);
});

test('entire constructor map is preflighted before any event constructor executes', async () => {
  const { viewer, calls } = await createInertReplayRuntime();
  const candidate = decode(literal());
  for (const provider of [null, {}, { getEventIdMap: 0 }]) {
    assert.throws(() => prepare(candidate, provider), { code: 'constructor-provider' });
  }
  for (const constructors of [null, undefined, 0, 'constructors']) {
    assert.throws(() => prepare(candidate, { getEventIdMap: () => constructors }), { code: 'constructor-map' });
  }
  let constructed = 0, accessed = 0;
  const map = { 0: function Prelude() { constructed++; } };
  Object.defineProperty(map, '255', { get() { accessed++; return function Conclusion() {}; } });
  assert.throws(() => prepare(candidate, { getEventIdMap: () => map }), { code: 'constructor-map' });
  assert.equal(constructed, 0); assert.equal(accessed, 0);
  assert.throws(() => prepare(candidate, { getEventIdMap: () => ({ 0: map[0] }) }), { code: 'constructor-map' });
  assert.equal(constructed, 0);
  assert.deepEqual(calls, []);
  assert.equal(prepare(candidate, viewer).events.length, 2);
});

test('exact framing, count bounds and header versions reject truncation, extra and overflow declarations', () => {
  assert.deepEqual(REPLAY_WIRE_LIMITS, { headerBytes: 64, toolBytes: 24, toolCount: 8,
    eventBytes: 16, eventsOffset: 256, maxEvents: 16384, maxBytes: 262400 });
  for (let length = 0; length < commandsWire.length; length++) {
    fails(commandsWire.subarray(0, length), length < 288 ? 'input-size' : 'length');
  }
  fails(new Uint8Array(262401), 'input-size');
  fails(Uint8Array.from([...emptyWire, 0]), 'length');
  fails(Uint8Array.from([...emptyWire, ...emptyWire]), 'length');
  for (const count of [0, 1, 16385, 0xffffffff]) {
    const bytes = literal(); view(bytes).setUint32(20, count); fails(bytes, 'event-count');
  }
  for (const length of [0, 287, 289, 0xffffffff]) {
    const bytes = literal(); view(bytes).setUint32(16, length); fails(bytes, 'length');
  }
  const wrongCount = literal(); view(wrongCount).setUint32(20, 3); fails(wrongCount, 'length');
  for (const offset of [...Array(16).keys(), 44, 45, 46, 47]) {
    const bytes = literal(); bytes[offset] ^= 1; fails(bytes, 'header');
  }
  // Exact maximum, with invalid lifecycle deliberately retained as structural.
  const max = framed([record(0), ...Array.from({ length: 16382 }, () => record(3)), record(255)]);
  assert.equal(max.length, 262400); assert.equal(decode(max).events.length, 16384);
  max[max.length - 1] = 1; fails(max, 'reserved');
});

test('all reserved fields and event-specific unused payload bytes must be zero', () => {
  for (const offset of [35, ...Array.from({ length: 16 }, (_, i) => 48 + i)]) fails(changed(literal(), offset, 1), 'reserved');
  for (let id = 0; id < 8; id++) for (let reserved = 16; reserved < 24; reserved++) {
    fails(changed(literal(), 64 + 24 * id + reserved, 1), 'reserved');
  }
  const used = tag => [1, 2].includes(tag) ? 6 : [7, 8, 12, 18, 27].includes(tag) ? 4
    : tag === 6 ? 3 : [10, 11, 13, 14, 15, 16, 17, 22, 24, 25, 26].includes(tag) ? 1 : 0;
  for (const tag of tags) {
    const bytes = tag === 0 || tag === 255 ? literal() : withEvent(tag, tag === 10 ? [1] : []);
    const offset = tag === 0 ? 256 : 272;
    for (const relative of [1, 2, 3, ...Array.from({ length: 8 - used(tag) }, (_, i) => 8 + used(tag) + i)]) {
      fails(changed(bytes, offset + relative, 1), 'reserved');
    }
  }
});

test('tool identity/order, metadata dimensions, booleans, marker placement and unknown tags fail closed', () => {
  for (const dimension of [0, 1025, 65535]) for (const offset of [24, 26]) {
    const bytes = literal(); view(bytes).setUint16(offset, dimension); fails(bytes, 'metadata');
  }
  for (const value of [0, 9, 255]) {
    fails(changed(literal(), 34, value), 'metadata'); fails(withEvent(10, [value]), 'tool');
  }
  for (let id = 0; id < 8; id++) {
    fails(changed(literal(), 64 + id * 24, 0), 'tool');
    fails(changed(literal(), 64 + id * 24, id === 0 ? 2 : 1), 'tool');
    for (const bit of [16, 32, 64, 128]) fails(changed(literal(), 66 + id * 24, bit), 'tool');
  }
  for (let flags = 0; flags < 16; flags++) assert.equal(decode(changed(literal(), 66, flags)).tools[0].flowDynamics, !!(flags & 8));
  for (const tag of [13, 14, 16, 17]) for (let value = 0; value < 256; value++) {
    if (value <= 1) assert.equal(decode(withEvent(tag, [value])).events[1].value, value);
    else fails(withEvent(tag, [value]), 'boolean');
  }
  for (let tag = 0; tag < 256; tag++) if (!tags.includes(tag)) fails(withEvent(tag), 'event-tag');
  for (const sequence of [[3, 255], [0, 3], [255, 0], [0, 0, 255], [0, 255, 255], [0, 0, 0, 255]]) {
    fails(framed(sequence.map(tag => record(tag))), 'markers');
  }
});

test('all tool and event float slots reject infinities and both quiet/signaling NaNs', () => {
  for (const bits of [0x7f800000, 0xff800000, 0x7fc00000, 0x7f800001, 0xff800001, 0xffffffff]) {
    for (let id = 0; id < 8; id++) for (const field of [4, 8, 12]) {
      const bytes = literal(); view(bytes).setUint32(64 + id * 24 + field, bits); fails(bytes, 'nonfinite-float');
    }
    for (const tag of [12, 18, 27]) {
      const bytes = withEvent(tag); view(bytes).setUint32(280, bits); fails(bytes, 'nonfinite-float');
    }
  }
});

test('inert oracle traps original decompression, loaders, dispatch, init and rendering APIs', async () => {
  const { viewer, calls } = await createInertReplayRuntime();
  const prepared = prepare(decode(commandsCore()), viewer);
  assert.deepEqual(calls, []);
  // Positive controls demonstrate traps are installed, after preparation ends.
  for (const name of ['loadFromURL', 'loadFromBuffer', 'decompressData', 'readMeta', 'readToolMap', 'readEventStack', 'play', 'step']) {
    assert.throws(() => viewer[name](), /Unexpected runtime activation/);
  }
  assert.throws(() => prepared.events[1].dispatch(), /Unexpected runtime activation/);
  assert.equal(calls.length, 9);
});

test('candidate module has no runtime decoding, browser, import, dynamic-code or activation path', async () => {
  const source = await readFile(new URL('../../apps/public/client/native-replay-data.js', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /\b(?:eval|Function|fetch|loadFromURL|loadFromBuffer|decompressData|UZIP|document|window|TegakiBinReader)\s*(?:\(|\.|\[)/);
  assert.doesNotMatch(source, /\bimport\s|\.dispatch\s*\(|\.init\s*\(|\.open\s*\(|\.play\s*\(/);
});
