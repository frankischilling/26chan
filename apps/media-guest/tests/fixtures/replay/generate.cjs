'use strict';
// Reproduce local synthetic fixtures with the unmodified production recorder.
// Verify: node generate.cjs [--source-file /path/to/tegaki.min.js]
// Legacy reference-root invocation remains supported.
// Record deliberate fixture changes: append --record
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const usage = 'usage: generate.cjs [--source-file FILE | REFERENCE_ROOT] [--record]';
const args = process.argv.slice(2);
let sourceFile = path.resolve(__dirname, '../../../../public/vendor/tegaki/0.9.4/tegaki.min.js');
let sourceSelected = false, record = false;
for (let index = 0; index < args.length; index++) {
  const argument = args[index];
  if (argument === '--record') {
    if (record) throw new Error(usage);
    record = true;
  } else if (argument === '--source-file') {
    const file = args[++index];
    if (sourceSelected || !file || file.startsWith('-')) throw new Error(usage);
    sourceSelected = true;
    sourceFile = path.resolve(file);
  } else {
    if (sourceSelected || !argument || argument.startsWith('-')) throw new Error(usage);
    sourceSelected = true;
    sourceFile = path.resolve(argument, 'js/tegaki.min.js');
  }
}
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const expectedSourceSha256 = 'daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744';
const expectedSourceBytes = 110619;
// Bound the read itself, and check the whole-file pin before any VM execution.
const sourceFd = fs.openSync(sourceFile, 'r');
let source;
try {
  const stat = fs.fstatSync(sourceFd);
  assert.ok(stat.isFile(), 'production source must be a regular file');
  assert.equal(stat.size, expectedSourceBytes, 'production source size changed');
  const buffer = Buffer.alloc(expectedSourceBytes + 1);
  let length = 0, received;
  do {
    received = fs.readSync(sourceFd, buffer, length, buffer.length - length, null);
    length += received;
  } while (received && length < buffer.length);
  assert.equal(length, expectedSourceBytes, 'production source size changed during read');
  source = buffer.subarray(0, length);
} finally {
  fs.closeSync(sourceFd);
}
assert.equal(sha256(source), expectedSourceSha256, 'production source hash changed');
let now = 1690000000000, perf = 123;
class FrozenDate extends Date { static now() { return now; } }
const sandbox = {Blob, Date: FrozenDate, performance: {now: () => perf}, navigator: {},
  document: {createElement() { throw Error('unexpected DOM call'); },
    body: {appendChild() {}, classList: {add() {}}}}};
sandbox.window = sandbox;
vm.createContext(sandbox);
vm.runInContext(source.toString(), sandbox, {timeout: 2000});
vm.runInContext(`
  Tegaki.initKeybinds = () => {};
  Tegaki.init = () => {};
  Tegaki.setTool = () => {};
  TegakiUI.buildUI = () => [{}, {}, {}];
  Tegaki.open({width: 640, height: 480, saveReplay: true});
`, sandbox, {timeout: 2000});
const fixtures = [];
const outputs = new Map();
function checkOrRecord(name, bytes) {
  outputs.set(name, Buffer.from(bytes));
}
function finish(name) {
  now += 12345; perf += 1000;
  const recorder = sandbox.Tegaki.replayRecorder;
  recorder.stop();
  const bytes = Buffer.from(recorder.toUint8Array());
  checkOrRecord(name + '.tgkr', bytes);
  fixtures.push({name, bytes: bytes.length, sha256: sha256(bytes),
    events: recorder.events.map(event => ({tag: event.type, timestamp: Math.trunc(event.timeStamp), size: event.size}))});
}
finish('empty');
vm.runInContext(`
  Tegaki.replayRecorder = new TegakiReplayRecorder();
  Tegaki.replayRecorder.start();
  const r = Tegaki.replayRecorder;
  let ts = 1200;
  // Synthetic editing commands packed by production classes, with real signed
  // off-canvas coordinates and pressure extrema. Nothing dispatches or renders.
  r.push(new TegakiEventSetColor(ts++, [12, 34, 56]));
  r.push(new TegakiEventSetTool(ts++, 2));
  r.push(new TegakiEventSetToolSize(ts++, 16));
  r.push(new TegakiEventSetToolAlpha(ts++, 0.5));
  r.push(new TegakiEventSetToolSizeDynamics(ts++, 1));
  r.push(new TegakiEventSetToolAlphaDynamics(ts++, 1));
  r.push(new TegakiEventPreserveAlpha(ts++, 1));
  r.push(new TegakiEventSetToolFlowDynamics(ts++, 1));
  r.push(new TegakiEventSetToolFlow(ts++, 0.25));
  r.push(new TegakiEventDrawStart(ts++, -3, 9, 0));
  r.push(new TegakiEventDraw(ts++, 12, -2, 65535));
  r.push(new TegakiEventDrawCommit(ts++));
  r.push(new TegakiEventUndo(ts++));
  r.push(new TegakiEventRedo(ts++));
  r.push(new TegakiEventSetTool(ts++, 8));
  r.push(new TegakiEventSetToolTip(ts++, 2));
  r.push(new TegakiEventDrawStartNoP(ts++, -32768, 32767));
  r.push(new TegakiEventDrawNoP(ts++, 640, 480));
  r.push(new TegakiEventDraw(ts++, 20, 30, 32768));
  r.push(new TegakiEventDrawCommit(ts++));
  r.push(new TegakiEventAddLayer(ts++));
  r.push(new TegakiEventToggleLayerVisibility(ts++, 2));
  r.push(new TegakiEventSetActiveLayer(ts++, 1));
  r.push(new TegakiEventToggleLayerSelection(ts++, 2));
  r.push(new TegakiEventSetSelectedLayersAlpha(ts++, 0.75));
  r.push(new TegakiEventMoveLayers(ts++, 3));
  r.push(new TegakiEventMergeLayers(ts++));
  r.push(new TegakiEventAddLayer(ts++));
  r.push(new TegakiEventDeleteLayers(ts++));
  r.push(new TegakiEventHistoryDummy(ts++));
`, sandbox, {timeout: 2000});
finish('commands');
checkOrRecord('manifest.json', JSON.stringify({
  sourceCommit: '545b7812d1849f7958d914950c91fdbbe38f6b22',
  sourceFile: 'js/tegaki.min.js', sourceSha256: expectedSourceSha256,
  description: 'Synthetic commands packed and compressed by the original Tegaki 0.9.4 recorder. No canvas, playback, or PNG correspondence tested.',
  fixtures
}, null, 2) + '\n');
// Generate all outputs successfully before any deliberate replacement.
for (const [name, bytes] of outputs) {
  const target = path.join(__dirname, name);
  if (record) fs.writeFileSync(target, bytes);
  else assert.deepEqual(fs.readFileSync(target), bytes, `fixture changed: ${name}`);
}
console.log(`verified ${outputs.size} production recorder fixtures`);
