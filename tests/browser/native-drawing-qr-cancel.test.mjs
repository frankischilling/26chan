import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { mountNativeDrawing } from '../../apps/public/client/native-drawing.js';

const source = await readFile(new URL('../../apps/public/client/native-quick-reply.js', import.meta.url), 'utf8');
const clear = /  function clearAttachment\(\) \{\n[^]*?\n  \}/.exec(source)?.[0];
const cancelListener = source.split('\n').find(line => line.includes("uploadCancel.addEventListener('click'"));
const shiftListener = source.split('\n').find(line => line.includes("uploadInput.addEventListener('click'"));

test('/i/ edit-only mounting requires its literal server flag and installs no ordinary Draw posting hooks', () => {
  const originalDocument = globalThis.document, originalWindow = globalThis.window;
  const events = { source: [], upload: [], file: [], window: [] };
  const makeControl = (key, tag = 'input') => ({ key, tag, value: '400', dataset: {}, style: {}, disabled: false,
    addEventListener() {}, removeAttribute() {}, cloneNode() { return makeControl(key, tag); } });
  const makeGroup = () => ({ dataset: {}, childNodes: [], addEventListener() {}, append(...nodes) {
    for (const node of nodes) { node.parentNode = this; this.childNodes.push(node); }
  },
    querySelector(selector) { return this.childNodes.find(node => node.key === selector) ?? null; },
    querySelectorAll(selector) { return selector === 'input' ? this.childNodes.filter(node => node.tag === 'input') : []; } });
  const makeText = text => ({ nodeType: 3, textContent: text,
    cloneNode() { return makeText(text); },
    remove() { const nodes = this.parentNode.childNodes; nodes.splice(nodes.indexOf(this), 1); },
  });
  const original = ['draw', 'clear', 'width', 'height', 'status'].map(name =>
    makeControl(`[data-drawing-${name}]`, ['draw', 'clear'].includes(name) ? 'button' : 'input'));
  const template = makeGroup();
  template.append(makeText('Size '), original[2], makeText(' × '), original[3], original[0], original[1], original[4]);
  const createSource = dataset => ({ dataset: { drawingWidth: '400', drawingHeight: '400', ...dataset },
    querySelector: selector => selector === '.painter-ctrl' ? template : null,
    elements: { namedItem: name => name === 'resto' ? { value: '100' } : { required: true } },
    querySelectorAll: () => [], addEventListener: type => events.source.push(type),
    append() {},
  });
  const ordinaryFile = { style: {}, disabled: false, addEventListener: type => events.file.push(type) };
  const uploadForm = { querySelector: () => ordinaryFile, addEventListener: type => events.upload.push(type) };
  const qrFiles = { style: {}, disabled: false, value: '' };
  const mounted = [];
  globalThis.document = { documentElement: { dataset: {} }, createElement: () => makeGroup() };
  globalThis.window = { addEventListener: type => events.window.push(type) };
  try {
    for (const board of ['qst', 'vip']) {
      assert.equal(mountNativeDrawing({ board, source: createSource({ drawingEditAllowed: 'true' }), uploadForm }), null,
        `${board} cannot mount an editor with only the /i/ Edit flag`);
    }
    assert.equal(mountNativeDrawing({ board: 'i', source: createSource({ drawingAllowed: 'true' }), uploadForm }), null,
      '/i/ ordinary Draw must remain disabled even if drawingAllowed is accidentally set');
    const edit = mountNativeDrawing({ board: 'i', source: createSource({ drawingEditAllowed: 'true' }), uploadForm });
    assert.ok(edit?.editAllowed);
    assert.deepEqual(events.source, [], 'no ordinary form submit/reset hooks');
    assert.deepEqual(events.upload, [], 'no ordinary upload form hooks');
    assert.deepEqual(events.file, [], 'no ordinary input hooks');
    const form = { querySelector: () => ({ append: controls => mounted.push(controls) }) };
    const controller = edit.mountQuickReply({ form, fileInput: qrFiles, key: 'quick-reply',
      target: () => '100', prepare: async () => true, accept: async () => true,
      clear: async () => true, approved: () => false });
    assert.ok(controller);
    assert.equal(mounted.length, 1);
    assert.equal(mounted[0].hidden, true, 'edit controls start hidden until import');
    assert.equal(mounted[0].querySelector('[data-drawing-draw]').hidden, true);
    assert.equal(mounted[0].querySelector('[data-drawing-width]').disabled, true);
    assert.equal(mounted[0].childNodes.filter(node => node.nodeType === 3).length, 0,
      'edit-only QR removes the ordinary Size and dimensions separator text');
    controller.dispose();

    for (const board of ['qst', 'vip']) {
      const ordinary = mountNativeDrawing({ board, source: createSource({ drawingAllowed: 'true', drawingEditAllowed: 'true' }), uploadForm });
      assert.ok(ordinary, `${board} retains ordinary Draw`);
      assert.equal(ordinary.editAllowed, false, `${board} never gains post Edit`);
    }
    assert.ok(events.source.includes('submit') && events.source.includes('reset'));
    assert.ok(events.upload.includes('submit') && events.file.includes('change'));
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
});

test('edit-only QR controls keep source metadata across Done/Edit and clear it on replacement, Cancel and Close', async () => {
  const drawingSource = await readFile(new URL('../../apps/public/client/native-drawing.js', import.meta.url), 'utf8');
  const controls = drawingSource.slice(drawingSource.indexOf('  function controlsFor('),
    drawingSource.indexOf('  const fileInput = ', drawingSource.indexOf('  function controlsFor(')));
  let client, approved = false, clearAllowed = false;
  const fields = new Map(), listeners = new Map();
  for (const name of ['draw', 'clear', 'width', 'height', 'status']) fields.set(`[data-drawing-${name}]`, {
    hidden: false, disabled: false, value: '400', textContent: '',
    addEventListener(name, fn) { listeners.set(`${this.key ?? 'field'}-${name}`, fn); },
  });
  const fileInput = { style: {}, disabled: false, value: '' }, group = {
    hidden: false, querySelector: selector => fields.get(selector),
  };
  const context = { painter: { active: () => false, invalidate() {}, retained: () => false,
    open(owner) { client = owner; }, importFromPost(owner, image) {
      client = owner;
      if (image.id === 'bad') { owner.loading(true); owner.error('Image could not be loaded'); owner.loading(false); return Promise.resolve(false); }
      owner.imported(image.id); return Promise.resolve(true);
    },
  } };
  vm.runInNewContext(controls, context, { timeout: 100 });
  const edit = context.controlsFor({}, group, { fileInput, key: 'quick-reply', target: () => '100',
    prepare: async () => true, accept: async () => true, clear: async () => clearAllowed,
    approved: () => approved, canDraw: false });
  assert.equal(group.hidden, true);
  assert.equal(fields.get('[data-drawing-draw]').hidden, true);
  assert.equal(fields.get('[data-drawing-width]').disabled, true);
  assert.equal(fields.get('[data-drawing-height]').hidden, true);
  assert.equal(await edit.importFromPost({ id: 'bad', url: 'https://media.example/i/missing.png' }), false);
  assert.equal(group.hidden, false, 'source image failure remains visible to the user');
  assert.equal(fields.get('[data-drawing-status]').textContent, 'Image could not be loaded');
  assert.equal(fields.get('[data-drawing-draw]').hidden, true, 'failure never offers blank Draw');
  client.loading(true);
  assert.equal(group.hidden, false, 'a retry shows a loading status');
  assert.equal(fields.get('[data-drawing-status]').textContent, 'Opening drawing…');
  client.loading(false);
  assert.equal(group.hidden, true, 'idle empty Edit controls return to their inert state');
  assert.equal(await edit.importFromPost({ id: '101', url: 'https://media.example/i/501.png' }), true);
  assert.equal(group.hidden, false);
  assert.equal(fields.get('[data-drawing-draw]').textContent, 'Edit');
  assert.equal(fields.get('[data-drawing-width]').disabled, true, 'successful import does not grant ordinary Draw');

  await client.finished({}, 2);
  assert.equal(edit.annotation(), null, 'an unapproved upload cannot carry source metadata');
  approved = true;
  assert.equal(JSON.stringify(edit.annotation()), '{"oe_src":"101","oe_time":"2"}');
  await client.finished({}, 17);
  assert.equal(edit.annotation().oe_time, '17', 'continued retained Edit refreshes elapsed seconds');
  assert.equal(await edit.clear(), false);
  assert.equal(edit.annotation().oe_src, '101', 'failed cancellation leaves the approved drawing owned');
  clearAllowed = true;
  assert.equal(await edit.clear(), true);
  assert.equal(edit.annotation(), null); assert.equal(group.hidden, true);

  await edit.importFromPost({ id: '102', url: 'https://media.example/i/502.png' });
  await client.finished({}, 3);
  assert.equal(edit.annotation().oe_src, '102');
  client.replaced();
  assert.equal(edit.annotation(), null, 'replacing the canvas retires its source');
  await edit.importFromPost({ id: '103', url: 'https://media.example/i/503.png' });
  await client.finished({}, 4);
  await client.cancelled();
  assert.equal(edit.annotation(), null, 'Tegaki Cancel removes its source metadata');
  await edit.importFromPost({ id: '104', url: 'https://media.example/i/504.png' });
  await client.finished({}, 5);
  edit.dispose();
  assert.equal(edit.annotation(), null, 'QR Close disposes the metadata owner');
});

// Execute the actual registered event handlers, not a parallel implementation.
// The drawing controller owns metadata clearing and retained-canvas semantics;
// cancelInlineUpload alone only retires the capability.
for (const route of ['button', 'shift-click']) test(`QR ${route} clears drawing metadata through the drawing Clear contract`, async () => {
  assert.ok(clear && cancelListener && shiftListener, 'actual QR attachment handler wiring exists');
  const calls = [], handlers = {};
  const drawing = { hasData: true, fileHidden: true, blocked: true, canvasRetained: true,
    pending() { return this.hasData; },
    async clear() {
      calls.push('drawing-clear'); this.hasData = false; this.fileHidden = false; this.blocked = false; return true;
    },
  };
  const context = { qrDrawing: drawing, cancelInlineUpload: async () => { calls.push('file-cancel'); return true; },
    uploadCancel: { addEventListener: (_, handler) => { handlers.button = handler; } },
    uploadInput: { addEventListener: (_, handler) => { handlers['shift-click'] = handler; } },
  };
  vm.runInNewContext(`${clear}\n${cancelListener}\n${shiftListener}`, context, { timeout: 100 });
  let prevented = false;
  handlers[route]({ shiftKey: true, preventDefault: () => { prevented = true; } });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear']); assert.equal(drawing.fileHidden, false); assert.equal(drawing.blocked, false);
  assert.equal(drawing.canvasRetained, true); if (route === 'shift-click') assert.equal(prevented, true);
  handlers[route]({ shiftKey: true, preventDefault() {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear', 'file-cancel'], 'ordinary files retain the existing cancellation flow');
  context.qrDrawing = null; handlers[route]({ shiftKey: true, preventDefault() {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear', 'file-cancel', 'file-cancel'], 'boards without drawing still cancel files');
});

test('ordinary file-picker clicks do not cancel an attachment', () => {
  let handler; const calls = [];
  vm.runInNewContext(`${clear}\n${shiftListener}`, { qrDrawing: { pending: () => true, clear: () => calls.push('drawing') },
    cancelInlineUpload: () => calls.push('file'), uploadInput: { addEventListener: (_, value) => { handler = value; } },
  }, { timeout: 100 });
  handler({ shiftKey: false, preventDefault: () => calls.push('prevented') }); assert.deepEqual(calls, []);
});

test('QR file picker combines drawing load/export with upload, status, cancel, and posting locks', async () => {
  const drawingSource = await readFile(new URL('../../apps/public/client/native-drawing.js', import.meta.url), 'utf8');
  const controls = drawingSource.slice(drawingSource.indexOf('  function controlsFor('),
    drawingSource.indexOf('  const fileInput = ', drawingSource.indexOf('  function controlsFor(')));
  const render = source.slice(source.indexOf('  function renderUpload()'), source.indexOf('  function resetInlineUpload()'));
  const mount = source.slice(source.indexOf('  function mountDrawing()'), source.indexOf('  function insert('));
  assert.match(controls, /function controlsFor/);
  assert.match(render, /function renderUpload/);
  assert.match(mount, /function mountDrawing/);

  const buttons = new Map();
  function button() {
    const listeners = new Map(), element = { disabled: false, textContent: '',
      addEventListener: (name, callback) => listeners.set(name, callback), listeners };
    return element;
  }
  for (const name of ['draw', 'clear', 'width', 'height', 'status']) buttons.set(`[data-drawing-${name}]`, button());
  buttons.get('[data-drawing-width]').value = '400';
  buttons.get('[data-drawing-height]').value = '400';
  const fileInput = { style: {}, disabled: false, value: '' };
  const context = {
    painter: { active: () => false, retained: () => false, invalidate() {},
      open(client) { context.editorClient = client; client.loading(true); } },
    drawing: null, qrDrawing: null,
    form: { querySelector: () => null }, uploadInput: fileInput,
    uploadStatus: { textContent: '' }, uploadCheck: {}, uploadCancel: {}, uploadSpoiler: null,
    uploadReceipt: null, uploadName: 'example.png', uploadPhase: 'empty', uploadCanCheck: false,
    uploadBusy: false, uploadOwned: false, busy: false, current: '42',
    closed: () => false, disabled: () => false, cancelAuto() {}, sync() {},
    cancelInlineUpload: async () => true, selectUpload: async () => true,
  };
  vm.runInNewContext(`${controls}\n${render}\n${mount}`, context, { timeout: 100 });
  context.drawing = { mountQuickReply: options => context.controlsFor(context.form,
    { querySelector: selector => buttons.get(selector) }, options) };
  context.mountDrawing();
  assert.ok(context.qrDrawing);
  assert.equal(fileInput.disabled, false);

  for (const phase of ['uploading', 'checking', 'canceling']) {
    context.uploadPhase = phase; context.uploadBusy = true;
    context.renderUpload();
    assert.equal(fileInput.disabled, true, `${phase} must disable the file picker`);
    context.qrDrawing.sync();
    assert.equal(fileInput.disabled, true, `${phase} cannot be undone by drawing render`);
    context.uploadBusy = false; context.renderUpload();
    assert.equal(fileInput.disabled, false, `${phase} completion restores the picker`);
  }
  context.busy = true; context.renderUpload();
  assert.equal(fileInput.disabled, true, 'posting disables the picker');
  context.busy = false; context.renderUpload();
  assert.equal(fileInput.disabled, false);

  buttons.get('[data-drawing-draw]').listeners.get('click')({ preventDefault() {} });
  assert.equal(fileInput.disabled, true, 'loading a drawing disables the picker');
  context.uploadBusy = true;
  context.editorClient.loading(false);
  assert.equal(fileInput.disabled, true, 'finishing a drawing load cannot unlock an ongoing upload');
  context.uploadBusy = false; context.renderUpload();
  assert.equal(fileInput.disabled, false);
  context.editorClient.exporting();
  context.renderUpload();
  assert.equal(fileInput.disabled, true, 'export keeps the picker disabled');
  assert.equal(fileInput.style.visibility, 'hidden');
  await context.qrDrawing.clear();
  assert.equal(fileInput.disabled, false, 'cleared drawing re-enables the picker when no work is pending');

  context.sync = () => { context.qrDrawing = null; context.uploadInput = null; context.uploadStatus = null; };
  assert.doesNotThrow(() => context.renderUpload(), 'a drawing render may close QR before upload rendering returns');
});

test('QR send attaches source annotation only to an owned /i/ drawing and retires it after success', async () => {
  const start = source.indexOf('  async function send(force = false) {');
  const end = source.indexOf('  function scheduleUploadCheck()', start);
  const send = source.slice(start, end);
  assert.ok(start > 0 && end > start);
  for (const [board, uploadOwned, annotated] of [['i', true, true], ['i', false, false], ['qst', true, false]]) {
    let delivered = null, resetCount = 0;
    const context = {
      board, busy: false, uploadBusy: false, uploadOwned, uploadReceipt: { state: 'approved' },
      dialog: {}, current: '100', epoch: 0, postingAttachment: false, controller: null, armedDraft: null,
      qrDrawing: { blocked: () => false, annotation: () => ({ oe_src: '101', oe_time: '9' }),
        reset() { resetCount++; } },
      drawing: { editAllowed: true }, form: {
        reportValidity: () => true, querySelector: () => null,
        elements: { namedItem: () => null },
      },
      FormData: class {
        *[Symbol.iterator]() { yield ['com', 'text']; yield ['upload_id', '1'.repeat(32)]; yield ['upload_capability', '2'.repeat(64)]; }
      },
      disabled: () => false, closed: () => false, cancelAuto() {},
      cooldown: { stop() {}, success() {} }, message() {},
      submit: { value: 'Post' }, renderUpload() {}, sync() {},
      sendQuickReply: async args => { delivered = args.fields; return { thread: '100', post: '102' }; },
      sourceApproval: () => null, resetInlineUpload() {},
      settings: () => ({ persistentQR: true }), comment: { value: 'text' },
      committed: async () => {},
      document: { dispatchEvent() {} }, Event: class { constructor(name) { this.type = name; } },
      AbortController,
    };
    vm.runInNewContext(send, context, { timeout: 100 });
    await context.send(true);
    assert.equal(delivered?.oe_src, annotated ? '101' : undefined, `${board}/${uploadOwned}: source ID`);
    assert.equal(delivered?.oe_time, annotated ? '9' : undefined, `${board}/${uploadOwned}: seconds`);
    assert.equal(resetCount, 1);
  }
});

// Use the real QR open function. A board index has multiple valid thread
// targets, and the QR painter retains a canvas even after its earlier form
// controller was closed or an upload attempt failed.
const qrOpen = source.slice(source.indexOf('  function open('), source.indexOf('  function mountDrawing()'));

function retargetContext({ ask = () => true, cancellation = async () => true } = {}) {
  assert.ok(qrOpen.startsWith('  function open(') && qrOpen.endsWith('  }\n'));
  const actions = [], label = { textContent: '10' }, resto = { value: '10' };
  const drawing = {
    retained: () => true,
    pending: () => false,
    dispose({ destroy }) { actions.push(['dispose', destroy]); },
  };
  const context = {
    source: {}, disabled: () => false, postId: value => value, closed: () => false,
    busy: false, targetEpoch: 0, dialog: {}, current: '10', uploadOwned: false,
    uploadBusy: false, qrDrawing: drawing,
    confirm(question) { actions.push(['confirm', question]); return ask(); },
    cancelAuto() { actions.push(['auto']); },
    cancelInlineUpload() { actions.push(['cancel-upload']); return cancellation(); },
    form: { elements: { resto } },
    document: { getElementById: id => { assert.equal(id, 'qrTid'); return label; } },
    comment: { value: 'keep this draft', focus() { actions.push(['focus']); } },
    mountDrawing() { actions.push(['mount']); },
    mobile: () => false,
  };
  vm.runInNewContext(qrOpen, context, { timeout: 100 });
  return { context, actions, label, resto, drawing };
}

test('QR retarget refusal preserves a retained canvas, target, and unsent text', () => {
  const { context, actions, label, resto } = retargetContext({ ask: () => false });
  assert.equal(context.open('11'), false);
  assert.equal(context.current, '10');
  assert.equal(resto.value, '10');
  assert.equal(label.textContent, '10');
  assert.equal(context.comment.value, 'keep this draft');
  assert.deepEqual(actions.map(([name]) => name), ['confirm']);
});

test('QR retarget accepts once before canceling an owned receipt and destroys the previous canvas', async () => {
  let resolveCancellation;
  const pending = new Promise(resolve => { resolveCancellation = resolve; });
  const { context, actions, label, resto } = retargetContext({ cancellation: () => pending });
  context.uploadOwned = true;
  assert.equal(context.open('11'), true);
  assert.equal(context.current, '10', 'wait for actual cancellation before switching targets');
  assert.deepEqual(actions.map(([name]) => name), ['confirm', 'auto', 'cancel-upload']);
  context.uploadOwned = false;
  resolveCancellation(true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(actions.filter(([name]) => name === 'confirm').length, 1, 'reentry must retain approval');
  assert.equal(context.current, '11');
  assert.equal(resto.value, '11');
  assert.equal(label.textContent, '11');
  assert.equal(context.comment.value, '');
  assert.ok(actions.some(([name, destroy]) => name === 'dispose' && destroy === true));
  assert.ok(actions.some(([name]) => name === 'mount'));
});

test('a declined retarget cannot invalidate an earlier confirmed pending cancellation', async () => {
  let resolveCancellation, consent = true;
  const pending = new Promise(resolve => { resolveCancellation = resolve; });
  const { context, actions } = retargetContext({ ask: () => consent, cancellation: () => pending });
  context.uploadOwned = true;
  assert.equal(context.open('11'), true);
  const pendingEpoch = context.targetEpoch;
  consent = false;
  assert.equal(context.open('12'), false);
  assert.equal(context.targetEpoch, pendingEpoch);
  assert.equal(context.current, '10');
  context.uploadOwned = false;
  resolveCancellation(true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(context.current, '11');
  assert.equal(actions.filter(([name]) => name === 'confirm').length, 2);
  assert.ok(actions.some(([name, destroy]) => name === 'dispose' && destroy === true));
});
