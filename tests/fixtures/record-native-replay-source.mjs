// Test-only reproducible oracle. Run with --record to deliberately replace the
// frozen JSON. This imports no adapter/decoder and never reads/decompresses TGKR.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createInertReplayRuntime, ownEventFields } from '../helpers/pinned-replay-runtime.mjs';

const args = process.argv.slice(2);
if (args.length > 1 || (args.length === 1 && args[0] !== '--record')) {
  throw new Error('usage: record-native-replay-source.mjs [--record]');
}

// Independent literal commands from the original recorder fixture transcript.
// These include all 28 structural tags, even the six preparation rejects.
const argumentsByTag = [
  [0, [1123]], [6, [1200, [12, 34, 56]]], [10, [1201, 2]], [11, [1202, 16]],
  [12, [1203, 0.5]], [13, [1204, 1]], [14, [1205, 1]], [16, [1206, 1]],
  [17, [1207, 1]], [18, [1208, 0.25]], [1, [1209, -3, 9, 0]],
  [2, [1210, 12, -2, 65535]], [3, [1211]], [4, [1212]], [5, [1213]],
  [10, [1214, 8]], [15, [1215, 2]], [7, [1216, -32768, 32767]],
  [8, [1217, 640, 480]], [2, [1218, 20, 30, 32768]], [3, [1219]],
  [20, [1220]], [24, [1221, 2]], [25, [1222, 1]], [26, [1223, 2]],
  [27, [1224, 0.75]], [22, [1225, 3]], [23, [1226]], [20, [1227]],
  [21, [1228]], [254, [1229]], [255, [2123]],
];
const runtime = await createInertReplayRuntime();
const constructors = runtime.viewer.getEventIdMap();
const fixture = {
  schema: 1, source: runtime.source,
  scope: 'Frozen own fields of actual production constructors, without dispatch or original decoding. No state, cost, lifecycle, raster, or playback approval.',
  wire: { path: 'tests/media/fixtures/replay-wire/commands.ibr', bytes: 768,
    sha256: 'ae72a8ee49b0a0b0f486b886d8a4d3b43708440b91b5954ee9efa62ce2bc26ce' },
  events: argumentsByTag.map(([tag, args]) => {
    const event = new constructors[tag](...args);
    return { tag, arguments: args, constructor: event.constructor.name, ownFields: ownEventFields(event) };
  }),
};
assert.deepEqual(runtime.calls, []);
const output = `${JSON.stringify(fixture, null, 2)}\n`;
const path = new URL('./native-replay-source.json', import.meta.url);
if (args.length) await writeFile(path, output);
else assert.equal(await readFile(path, 'utf8'), output, 'Frozen source oracle changed');
