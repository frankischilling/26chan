import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';

const oracle = JSON.parse(await readFile(new URL('../fixtures/native-drawing-source.json', import.meta.url)));
const snippets = Object.fromEntries(Object.entries(oracle.snippets).map(([name, value]) => [name, value.text]));

test('drawing oracle pins independent production source bytes, not implementation output', () => {
  assert.equal(oracle.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  for (const row of Object.values(oracle.snippets)) {
    assert.equal(Buffer.byteLength(row.text), row.byte_end - row.byte_start);
    assert.equal(createHash('sha256').update(row.text).digest('hex'), row.sha256);
    assert.match(oracle.files[row.file], /^[a-f0-9]{64}$/);
  }
  assert.equal(oracle.snippets.tegaki_done_cancel.file, 'js/tegaki.min.js');
  assert.match(oracle.scope, /Actual raster output requires the real isolated drawing browser lane/);
});

test('source defaults to 400 on i/qst/vip only and replay only on i; i bounds are separate constants', () => {
  assert.deepEqual(oracle.policies['config/global_config.ini'], [
    'ENABLE_PAINTERJS = no', 'PAINTERJS_DIMS = 400', 'ENABLE_OEKAKI_REPLAYS = no',
  ]);
  for (const board of ['i', 'qst', 'vip']) assert.ok(oracle.policies[`config/boards/${board}.config.ini`].includes('ENABLE_PAINTERJS = yes'));
  assert.deepEqual(oracle.policies['config/boards/qst.config.ini'], ['ENABLE_PAINTERJS = yes']);
  assert.deepEqual(oracle.policies['config/boards/vip.config.ini'], ['ENABLE_PAINTERJS = yes']);
  assert.ok(oracle.policies['config/boards/i.config.ini'].includes('ENABLE_OEKAKI_REPLAYS = yes'));
  assert.ok(oracle.policies['config/boards/test.config.ini'].includes(';ENABLE_PAINTERJS = yes'));
  for (const axis of ['W', 'H']) {
    assert.ok(oracle.policies['config/boards/i.config.ini'].includes(`OEKAKI_MIN_${axis} = 100`));
    assert.ok(oracle.policies['config/boards/i.config.ini'].includes(`OEKAKI_MAX_${axis} = 800`));
  }
  assert.doesNotMatch(snippets.ordinary + snippets.qr, /OEKAKI_(MIN|MAX)/);
  assert.match(snippets.template, /data-type="Painter" class="desktop"/);
  assert.match(snippets.template, /if \(ENABLE_OEKAKI_REPLAYS\)/);
});

test('actual pinned Tegaki Finish hides and retains; editor Cancel confirms before destroy and callback', () => {
  const calls = [];
  let accepted = false;
  const context = { Tegaki: { hide: () => calls.push('hide'), destroy: () => calls.push('destroy'),
    onDoneCb: () => calls.push('done'), onCancelCb: () => calls.push('cancel') },
    TegakiStrings: { confirmCancel: 'Are you sure? Your work will be lost.' },
    confirm: message => { calls.push(message); return accepted; } };
  const methods = vm.runInNewContext(`({${snippets.tegaki_done_cancel}})`, context, { timeout: 100 });
  methods.onDoneClick();
  assert.deepEqual(calls.splice(0), ['hide', 'done']);
  methods.onCancelClick();
  assert.deepEqual(calls.splice(0), ['Are you sure? Your work will be lost.']);
  accepted = true; methods.onCancelClick();
  assert.deepEqual(calls, ['Are you sure? Your work will be lost.', 'destroy', 'cancel']);
});

test('production Tegaki resumes retained canvas before replacing dimensions or callbacks', () => {
  const calls = [];
  const done = () => {};
  const Tegaki = { bg: {}, replayMode: false, baseWidth: 400, baseHeight: 400,
    onDoneCb: done, resume: () => calls.push('resume'), destroy: () => calls.push('destroy') };
  const methods = vm.runInNewContext(`({${snippets.tegaki_open}})`, { Tegaki }, { timeout: 100 });
  methods.open({ width: 123, height: 456, onDone: () => {}, replayMode: false });
  assert.deepEqual(calls, ['resume']);
  assert.equal(Tegaki.baseWidth, 400); assert.equal(Tegaki.baseHeight, 400); assert.equal(Tegaki.onDoneCb, done);
});

test('source ordinary Draw accepts positive dimensions, suspends keys, exports PNG and Clear leaves canvas', () => {
  let options;
  const fields = [{ value: '400', disabled: false }, { value: '400', disabled: false }];
  const controls = { getElementsByTagName: () => fields };
  const formEvents = [];
  const context = { window: {}, Keybinds: { enabled: true },
    document: { forms: { post: { addEventListener: (...value) => formEvents.push(['add', ...value]),
      removeEventListener: (...value) => formEvents.push(['remove', ...value]) } } },
    Tegaki: { open: value => { options = value; }, flatten: () => ({ toDataURL: type => `data:${type},owned` }),
      hasCustomCanvas: false, startTimeStamp: 0, saveReplay: false,
      destroy: () => assert.fail('Form Clear must not destroy retained source canvas') } };
  context.window.Keybinds = context.Keybinds;
  vm.createContext(context); vm.runInContext(snippets.ordinary, context, { timeout: 100 });
  const core = context.PainterCore;
  Object.assign(core, { btnFile: { disabled: false, style: {} }, btnClear: {}, btnDraw: {}, inputNodes: fields });
  core.onDrawClick.call({ parentNode: controls });
  assert.equal(options.width, 400); assert.equal(options.height, 400); assert.equal(context.Keybinds.enabled, false);
  core.onDone();
  assert.equal(core.data, 'data:image/png,owned'); assert.equal(core.btnDraw.textContent, 'Edit');
  assert.equal(core.btnFile.disabled, true); assert.equal(fields[0].disabled, true); assert.equal(context.Keybinds.enabled, true);
  core.onCancel();
  assert.equal(core.data, null); assert.equal(core.btnDraw.textContent, 'Draw'); assert.equal(core.btnClear.disabled, true);
  assert.equal(core.btnFile.disabled, false); assert.equal(fields[0].disabled, false);
  assert.deepEqual(formEvents.map(value => value[0]), ['add', 'remove']);
  options = null; fields[0].value = '0'; core.onDrawClick.call({ parentNode: controls }); assert.equal(options, null);
});

test('source QR keeps separate draft export state, restores keys, and drops upload state when closed', () => {
  assert.match(snippets.qr, /QR\.painterData = Tegaki\.flatten\(\)\.toDataURL\('image\/png'\)/);
  assert.match(snippets.qr, /Keybinds\.enabled = false/);
  assert.match(snippets.qr, /Keybinds\.enabled = true/);
  assert.match(snippets.qr, /el\.textContent = 'Edit'/);
  assert.match(snippets.qr, /el\.textContent = 'Draw'/);
  assert.doesNotMatch(snippets.qr, /Tegaki\.destroy/);
  assert.match(snippets.qr_close, /QR\.painterData = null/);
  assert.match(snippets.qr_close, /QR\.currentTid = null/);
  assert.match(snippets.qr_close, /QR\.xhr\.abort\(\)/);
});
