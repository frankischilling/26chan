import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { drawingDimensions, drawingPngFile, createDrawingUpload } from '../../apps/public/client/native-drawing-core.js';
import { createDrawingPainter, createDrawingDownload } from '../../apps/public/client/native-drawing-painter.js';
import { drawingPostingResult, postingResult, parseQuickReplyUpload, uploadTarget, sendDrawingPost,
  sendQuickReply, uploadQuickReplyFile } from '../../apps/public/client/native-quick-reply-transport.js';

const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
const receipt = (resto = '0', state = 'queued', id = '1') => ({ upload_id: id.repeat(32), upload_capability: '2'.repeat(64), resto, state });
function png(width = 400, height = 400) {
  const bytes = new Uint8Array(33); bytes.set([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]);
  const view = new DataView(bytes.buffer); view.setUint32(16, width); view.setUint32(20, height);
  return new Blob([bytes], { type: 'image/png' });
}
function editor() {
  return { bg: null, baseWidth: 0, baseHeight: 0, opens: 0, destroys: 0, hides: 0,
    open(options) { this.opens++; if (this.bg) return; this.bg = {}; this.baseWidth = options.width; this.baseHeight = options.height; this.onDoneCb = options.onDone; this.onCancelCb = options.onCancel; assert.equal(options.saveReplay, false); assert.equal(options.replayMode, false); },
    flatten() { return { width: this.baseWidth, height: this.baseHeight, toBlob: callback => callback(png(this.baseWidth, this.baseHeight)) }; },
    hide() { this.hides++; }, destroy() { this.destroys++; this.bg = null; },
    resizeCanvas(w, h) { this.baseWidth = w; this.baseHeight = h; },
    onNewClick(w, h) { this.resizeCanvas(w, h); },
  };
}
function client(key = 'ordinary', target = '0') {
  const state = { pending: false, disposed: false, files: [], errors: [], clears: 0 };
  return { key, target, state, disposed: () => state.disposed, pending: () => state.pending,
    loading(value) { state.loading = value; }, error(value) { state.errors.push(value); },
    exporting() { state.pending = true; }, prepare: async () => true,
    finished(file) { state.files.push(file); }, cancelled() { state.pending = false; },
    clearForReplacement: async () => { state.clears++; state.pending = false; return true; },
    clear: async () => { state.clears++; state.pending = false; return true; }, replaced() { state.pending = false; },
  };
}

test('canvas bounds are finite canonical whole pixels, with the decoder safety boundary', () => {
  assert.deepEqual(drawingDimensions('1', '1024'), { width: 1, height: 1024 });
  assert.deepEqual(drawingDimensions(1024, 1024), { width: 1024, height: 1024 });
  for (const value of ['0', '-1', '01', '1e2', '', 'Infinity', Infinity, NaN, 1.5, 1025, null, undefined, true]) {
    assert.throws(() => drawingDimensions(value, 400)); assert.throws(() => drawingDimensions(400, value));
  }
});

test('PNG export checks MIME, dimensions, bytes and signature before upload', async () => {
  const canvas = { width: 400, height: 400, toBlob: callback => callback(png()) };
  const file = await drawingPngFile(canvas); assert.equal(file.name, 'tegaki.png'); assert.equal(file.type, 'image/png');
  for (const blob of [null, new Blob(['html'], { type: 'text/html' }), png(401, 400), new Blob([new Uint8Array(33)], { type: 'image/png' })]) {
    await assert.rejects(drawingPngFile({ ...canvas, toBlob: callback => callback(blob) }));
  }
  let exports = 0;
  await assert.rejects(drawingPngFile({ ...canvas, width: 1025, toBlob: () => exports++ })); assert.equal(exports, 0);
});

test('zero is admitted only for ordinary upload and drawing posts; replies retain positive target checks', async () => {
  assert.equal(uploadTarget('0'), true); assert.equal(uploadTarget('10'), true);
  for (const invalid of [0, -1, '00', '01', '', null, '9223372036854775808']) assert.equal(uploadTarget(invalid), false);
  assert.deepEqual(parseQuickReplyUpload(JSON.stringify(receipt()), '0'), receipt());
  assert.throws(() => parseQuickReplyUpload(JSON.stringify(receipt('10')), '0'));
  assert.deepEqual(drawingPostingResult('{"tid":0,"pid":9223372036854775807}', '0'), { thread: '9223372036854775807', post: '9223372036854775807' });
  assert.throws(() => postingResult('{"tid":0,"pid":10}', '0'));
  for (const text of ['{"tid":10,"pid":11}', '{"tid":0,"pid":0}', '{"tid":0,"pid":1,"extra":true}']) assert.throws(() => drawingPostingResult(text, '0'));
  let calls = 0;
  await assert.rejects(sendQuickReply({ board: 'qst', thread: '0', fields: {}, origin: 'https://board.test', fetcher: () => calls++ }));
  assert.equal(calls, 0);
  const result = await sendDrawingPost({ board: 'qst', thread: '0', origin: 'https://board.test', fields: { sub: 'A subject', com: 'draft', upload_id: receipt().upload_id, upload_capability: receipt().upload_capability, upfile: new Blob(['raw']) }, fetcher: async (url, options) => {
    assert.equal(url, 'https://board.test/qst/imgboard.php'); assert.equal(options.body.get('resto'), '0');
    assert.equal(options.body.get('sub'), 'A subject'); assert.equal(options.body.get('com'), 'draft');
    assert.equal(options.body.has('upfile'), false); assert.equal(options.body.has('oe_replay'), false);
    assert.equal(options.body.get('upload_capability'), receipt().upload_capability);
    return new Response('{"tid":0,"pid":42}', { headers: { 'content-type': 'application/json' } });
  } });
  assert.deepEqual(result, { thread: '42', post: '42' });
  const file = new File([png()], 'tegaki.png', { type: 'image/png' });
  await uploadQuickReplyFile({ board: 'vip', thread: '0', origin: 'https://board.test', file, fetcher: async (url, options) => {
    assert.equal(url, 'https://board.test/vip/upload'); assert.equal(options.body.get('resto'), '0');
    return new Response(JSON.stringify(receipt()), { headers: { 'content-type': 'application/json' } });
  } });
});

test('upload approval is owned, isolated and canceled before replacement', async () => {
  const calls = [], states = [];
  let nextId = '1';
  const upload = createDrawingUpload({ board: 'qst', target: () => '0', changed: state => states.push(state), schedule: () => 0, unschedule: () => {},
    upload: async () => { calls.push('upload'); return receipt('0', 'queued', nextId); },
    check: async ({ receipt: value }) => ({ ...value, state: 'approved' }),
    cancel: async ({ receipt: value }) => { calls.push(`cancel-${value.upload_id[0]}`); },
  });
  await upload.select('first'); assert.equal(upload.snapshot().approved, false);
  await upload.status(); assert.equal(upload.snapshot().approved, true);
  nextId = '3'; await upload.select('second'); assert.deepEqual(calls, ['upload', 'cancel-1', 'upload']);
  assert.equal(upload.snapshot().receipt.upload_id, '3'.repeat(32));
  await upload.clear(); assert.equal(upload.snapshot().pending, false); assert.equal(upload.snapshot().receipt, null);
  assert.ok(states.some(state => state.phase === 'checking')); upload.dispose();
});

test('failed cancellation preserves the old capability and stops replacement', async () => {
  let calls = 0;
  const upload = createDrawingUpload({ board: 'qst', target: () => '10', schedule: () => 0, unschedule: () => {},
    upload: async () => { calls++; return receipt('10', 'approved'); }, cancel: async () => { throw new Error('Cannot cancel'); },
  });
  await upload.select('first'); assert.equal(await upload.select('second'), false);
  assert.equal(calls, 1); assert.equal(upload.snapshot().receipt.upload_id, '1'.repeat(32)); assert.equal(upload.snapshot().error, 'Cannot cancel');
  upload.posting(true); upload.dispose();
});

test('late uploads and status cannot attach after clear, new target, disposal or replacement', async () => {
  const first = deferred(), checks = deferred(), cancelled = [];
  let target = '10', call = 0;
  const upload = createDrawingUpload({ board: 'qst', target: () => target, schedule: () => 0, unschedule: () => {},
    upload: () => ++call === 1 ? first.promise : Promise.resolve(receipt(target, 'queued', '3')),
    check: () => checks.promise, cancel: async ({ receipt: value, thread }) => cancelled.push([value.upload_id[0], thread]),
  });
  const old = upload.select('old'); await tick(); await upload.clear();
  first.resolve(receipt('10')); assert.equal(await old, false); assert.equal(upload.snapshot().receipt, null);
  assert.deepEqual(cancelled, [['1', '10']]);
  await upload.select('new'); const status = upload.status(); target = '11'; upload.resetTarget();
  checks.resolve(receipt('10', 'approved', '3')); assert.equal(await status, false); assert.equal(upload.snapshot().receipt, null);
  await upload.select('latest'); upload.dispose(); assert.deepEqual(cancelled.at(-1), ['3', '11']);
});

test('concurrent replacements select only the latest file after one cancellation', async () => {
  const cancellation = deferred(), sent = [];
  const upload = createDrawingUpload({ board: 'qst', target: () => '10', schedule: () => 0, unschedule: () => {},
    upload: async ({ file }) => { sent.push(file); return receipt('10'); }, cancel: () => cancellation.promise,
  });
  await upload.select('initial'); const first = upload.select('obsolete'), second = upload.select('latest');
  cancellation.resolve(); assert.equal(await first, false); assert.equal(await second, true);
  assert.deepEqual(sent, ['initial', 'latest']); upload.posting(true); upload.dispose();
});

test('submitting a one-use attachment prevents pagehide cancellation; retirement never reuses it', async () => {
  let cancels = 0;
  const upload = createDrawingUpload({ board: 'vip', target: () => '0', upload: async () => receipt('0', 'approved'), cancel: async () => cancels++ });
  await upload.select('png'); upload.posting(true); assert.equal(await upload.clear(), false); upload.dispose(); assert.equal(cancels, 0);
  const second = createDrawingUpload({ board: 'vip', target: () => '0', upload: async () => receipt('0', 'approved'), cancel: async () => cancels++ });
  await second.select('png'); second.posting(true); second.retire(); assert.equal(second.snapshot().receipt, null); second.dispose(); assert.equal(cancels, 0);
});

test('Finish and Clear retain real editor state; Edit rebinds callbacks and editor Cancel destroys first', async () => {
  const engine = editor(), active = [], drawing = createDrawingPainter({ load: async () => engine, activeChanged: value => active.push(value) }), owner = client();
  assert.equal(await drawing.open(owner, '400', '400'), true); await engine.onDoneCb();
  assert.equal(owner.state.files.length, 1); assert.ok(engine.bg); assert.equal(drawing.active(), false);
  drawing.invalidate(owner); owner.state.pending = false; assert.equal(engine.destroys, 0);
  assert.equal(await drawing.open(owner, '500', '500'), true); assert.equal(engine.baseWidth, 400);
  engine.destroy(); engine.onCancelCb(); assert.equal(owner.state.pending, false); assert.equal(drawing.active(), false);
  assert.deepEqual(active, [true, false, false, true, false]); drawing.dispose();
});

test('qualification waits for Edit to reopen before sending the revoked receipt', async () => {
  const source = await readFile(new URL('./drawing-upload.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('  await controls.draw.click();', source.indexOf('const firstReceipt ='));
  const end = source.indexOf("  await signal('REVOKED', firstReceipt.upload_id);", start);
  assert.ok(start > 0 && end > start);
  const fragment = source.slice(start, end) + "  await signal('REVOKED', firstReceipt.upload_id);";
  const run = new Function('controls', 'page', 'expect', 'canvasProof', 'firstProof', 'signal', 'firstReceipt', 'assert',
    `return (async () => { ${fragment} })();`);
  const visible = deferred(), signals = [], proof = { hash: 'retained-canvas' };
  const work = run({ draw: { click: async () => {} } },
    { locator: selector => { assert.equal(selector, '#tegaki-cursor-layer'); return selector; } },
    () => ({ toBeVisible: () => visible.promise }), async () => proof, proof,
    async (...args) => signals.push(args), receipt(), assert);
  await tick();
  assert.deepEqual(signals, [], 'reading a retained canvas cannot prove cancellation');
  visible.resolve(); await work;
  assert.deepEqual(signals, [['REVOKED', receipt().upload_id]]);
});

for (const succeeds of [true, false]) test(`Edit only reopens retained canvas after successful receipt cancellation (${succeeds})`, async () => {
  const cancellation = deferred(), engine = editor(), owner = client();
  const upload = createDrawingUpload({ board: 'qst', target: () => '0',
    upload: async () => receipt(), cancel: () => cancellation.promise,
    schedule: () => 0, unschedule() {},
  });
  owner.prepare = () => upload.clear();
  owner.finished = file => upload.select(file);
  const drawing = createDrawingPainter({ load: async () => engine });
  try {
    assert.equal(await drawing.open(owner, 400, 400), true);
    await engine.onDoneCb();
    const retained = engine.bg, canvas = engine.flatten();
    assert.equal(upload.snapshot().phase, 'queued');
    const edit = drawing.open(owner, 400, 400);
    await tick();
    // This is the former harness race: canvas dimensions/pixels remain
    // readable even though the old receipt has not been revoked yet.
    assert.equal(engine.bg, retained);
    assert.equal(engine.flatten().width, canvas.width);
    assert.equal(engine.flatten().height, canvas.height);
    assert.equal(upload.snapshot().phase, 'canceling');
    assert.equal(upload.snapshot().receipt.upload_id, receipt().upload_id);
    assert.equal(drawing.active(), false);
    assert.equal(engine.opens, 1, 'retained editor has not reopened during cancellation');
    if (succeeds) cancellation.resolve();
    else cancellation.reject(new Error('Cancellation rejected'));
    assert.equal(await edit, succeeds);
    assert.equal(drawing.active(), succeeds);
    assert.equal(engine.opens, succeeds ? 2 : 1);
    assert.equal(engine.bg, retained, 'success and failure both retain the source canvas');
    if (succeeds) assert.equal(upload.snapshot().receipt, null);
    if (!succeeds) {
      assert.equal(upload.snapshot().receipt.upload_id, receipt().upload_id);
      assert.equal(upload.snapshot().error, 'Cancellation rejected');
      assert.equal(upload.snapshot().phase, 'queued');
    }
  } finally {
    drawing.dispose(); upload.posting(true); upload.dispose();
  }
});

test('export and editor-load completions are stale after Clear or pagehide', async () => {
  const engine = editor(), exported = deferred(), drawing = createDrawingPainter({ load: async () => engine, exportFile: () => exported.promise }), owner = client();
  await drawing.open(owner, 400, 400); const finish = engine.onDoneCb(); drawing.invalidate(owner); owner.state.pending = false;
  exported.resolve(new File([png()], 'tegaki.png', { type: 'image/png' })); await finish; assert.equal(owner.state.files.length, 0);
  drawing.dispose(); assert.equal(engine.destroys, 1);
  const pending = deferred(), next = createDrawingPainter({ load: () => pending.promise }), another = client();
  const open = next.open(another, 400, 400); next.dispose(); pending.resolve(engine); assert.equal(await open, false); assert.equal(engine.opens, 1);
});

test('QR close/reopen keeps its same-target canvas, while other form replacement asks and cancels ownership', async () => {
  const engine = editor(); let allowed = false, confirms = 0;
  const drawing = createDrawingPainter({ load: async () => engine, confirmReplacement: () => { confirms++; return allowed; } });
  const first = client('quick-reply', '10'); await drawing.open(first, 400, 400); await engine.onDoneCb();
  drawing.invalidate(first); first.state.disposed = true; first.state.pending = false;
  const reopened = client('quick-reply', '10'); await drawing.open(reopened, 400, 400); await engine.onDoneCb();
  assert.equal(engine.destroys, 0); assert.equal(reopened.state.files.length, 1);
  const other = client('ordinary'); assert.equal(await drawing.open(other, 400, 400), false); assert.equal(confirms, 1);
  allowed = true; assert.equal(await drawing.open(other, 500, 500), true); assert.equal(reopened.state.clears, 1);
  assert.equal(engine.destroys, 1); assert.equal(engine.baseWidth, 500); drawing.dispose();
});

test('invalid form dimensions never load the editor; internal New is bounded before allocating', async () => {
  let loaded = 0; const engine = editor(), drawing = createDrawingPainter({ load: async () => { loaded++; return engine; } }), owner = client();
  assert.equal(await drawing.open(owner, Infinity, 400), false); assert.equal(loaded, 0);
  await drawing.open(owner, 400, 400); engine.onNewClick(1025, 400); assert.equal(engine.baseWidth, 400);
  engine.onNewClick(1024, 1024); assert.equal(engine.baseWidth, 1024); drawing.dispose();
});


test('owned PNG downloads revoke on replacement, expiry, dismissal and error with stale timers fenced', async () => {
  const created = [], revoked = [], sent = [], timers = [];
  const downloads = createDrawingDownload({ exportFile: async () => new File([png()], 'tegaki.png', { type: 'image/png' }),
    createURL: blob => { assert.equal(blob.type, 'image/png'); const url = `blob:owned-${created.length}`; created.push(url); return url; },
    revokeURL: url => revoked.push(url), download: (url, name) => sent.push([url, name]),
    schedule: (callback, delay) => { assert.equal(delay, 30000); timers.push(callback); return timers.length; }, unschedule: () => {},
  });
  assert.equal(await downloads.save({}), true); assert.deepEqual(sent, [['blob:owned-0', 'tegaki.png']]);
  assert.equal(await downloads.save({}), true); assert.deepEqual(revoked, ['blob:owned-0']);
  timers[0](); assert.deepEqual(revoked, ['blob:owned-0'], 'an already queued old timer cannot revoke the current URL');
  timers[1](); assert.deepEqual(revoked, ['blob:owned-0', 'blob:owned-1']);
  await downloads.save({}); downloads.invalidate(); assert.equal(revoked.at(-1), 'blob:owned-2');
  await downloads.save({}); downloads.dispose(); assert.equal(revoked.at(-1), 'blob:owned-3');
  assert.equal(await downloads.save({}), false); assert.equal(created.length, 4);
  const error = createDrawingDownload({ exportFile: async () => new File([png()], 'tegaki.png', { type: 'image/png' }),
    createURL: () => 'blob:failed', revokeURL: url => revoked.push(url), download: () => { throw new Error('download blocked'); },
  });
  await assert.rejects(error.save({}), /download blocked/); assert.equal(revoked.at(-1), 'blob:failed'); error.dispose();
});

test('pending PNG download is single-flight and cannot create a URL after Clear, replacement or pagehide', async () => {
  for (const operation of ['invalidate', 'dispose']) {
    const pending = deferred(); let exports = 0, urls = 0;
    const downloads = createDrawingDownload({ exportFile: () => { exports++; return pending.promise; }, createURL: () => { urls++; return 'blob:unexpected'; } });
    const first = downloads.save({}); assert.equal(downloads.busy(), true); assert.equal(await downloads.save({}), false); assert.equal(exports, 1);
    downloads[operation](); pending.resolve(new File([png()], 'tegaki.png', { type: 'image/png' }));
    assert.equal(await first, false); assert.equal(urls, 0); assert.equal(downloads.busy(), false); downloads.dispose();
  }
  const pending = deferred(); let urls = 0, current = true;
  const downloads = createDrawingDownload({ exportFile: () => pending.promise, createURL: () => { urls++; return 'blob:unexpected'; } });
  const saved = downloads.save({}, () => current); current = false; pending.resolve(new File([png()], 'tegaki.png', { type: 'image/png' }));
  assert.equal(await saved, false); assert.equal(urls, 0); downloads.dispose();
});

test('editor Open and its picker are explicitly disabled without invoking the source image decoder', async () => {
  const engine = editor(), owner = client(), attributes = {}, classes = [];
  const openButton = { dataset: {}, setAttribute: (key, value) => { attributes[key] = value; }, classList: { add: value => classes.push(value) } }, picker = {};
  const open = engine.open;
  engine.open = function(options) { open.call(this, options); this.bg.querySelector = selector => selector === '#tegaki-filepicker' ? picker : { querySelectorAll: () => [{}, openButton] }; };
  engine.onOpenClick = engine.onOpenFileSelected = () => { throw new Error('unbounded import must not run'); };
  const drawing = createDrawingPainter({ load: async () => engine });
  await drawing.open(owner, 400, 400);
  assert.equal(openButton.textContent, 'Open (unavailable)'); assert.equal(attributes['aria-disabled'], 'true'); assert.equal(picker.disabled, true);
  assert.deepEqual(classes, ['tegaki-disabled']); assert.equal(engine.onOpenClick(), false); assert.equal(engine.onOpenFileSelected(), false);
  assert.equal(owner.state.errors.at(-1), 'Opening an image in the drawing editor is unavailable.'); drawing.dispose();
});

test('Finish and confirmed Cancel invalidate pending download independently of upload export', async () => {
  for (const action of ['finish', 'cancel']) {
    const pending = deferred(), engine = editor(), owner = client(); let urls = 0;
    const downloads = createDrawingDownload({ exportFile: () => pending.promise, createURL: () => { urls++; return 'blob:unexpected'; } });
    const drawing = createDrawingPainter({ load: async () => engine, downloads });
    await drawing.open(owner, 400, 400); const exported = engine.onExportClick();
    if (action === 'finish') { await engine.onDoneCb(); assert.equal(owner.state.files.length, 1); }
    else { engine.destroy(); engine.onCancelCb(); assert.equal(owner.state.files.length, 0); }
    pending.resolve(new File([png()], 'tegaki.png', { type: 'image/png' })); await exported;
    assert.equal(urls, 0); drawing.dispose();
  }
});


test('Edit diagnostics classify real transport and UI without leaking bodies or capabilities', async () => {
  const { observeDrawingCancellation, drawingStatusCategory, reportDrawingEditFailure } = await import('./helpers/drawing-browser.mjs');
  const handlers = {}, lines = [], url = 'http://127.0.0.1/owned/upload/cancel';
  const request = { url: () => url, method: () => 'POST', postData: () => { throw new Error('must not read capability'); } };
  const page = { on: (name, callback) => { handlers[name] = callback; }, locator: selector => ({
    count: async () => 1, isVisible: async () => false, getAttribute: async () => 'false',
  }) };
  const cancellation = observeDrawingCancellation(page, url);
  assert.equal(cancellation(), 'none'); handlers.request(request); assert.equal(cancellation(), 'pending');
  handlers.response({ url: () => url, request: () => request, status: () => 403 });
  assert.equal(cancellation(), '403');
  for (const text of ['secret-capability', '__proto__', 'constructor', 'cancel-error\nsecret']) assert.equal(drawingStatusCategory(text), 'other');
  await reportDrawingEditFailure(page, { status: { textContent: async () => 'secret-capability' } }, cancellation, value => lines.push(value));
  assert.deepEqual(lines, ['OWNED_DRAWING_EDIT cancel=403 ui=other editor=hidden cursor=hidden active=false']);
  handlers.requestfailed(request); assert.equal(cancellation(), '403', 'body disposal must not erase an observed HTTP response');
  handlers.request(request); handlers.requestfailed(request); assert.equal(cancellation(), 'failed');
  await reportDrawingEditFailure(page, { status: { textContent: async () => { throw new Error('secret'); } } }, cancellation, value => lines.push(value));
  assert.equal(lines.at(-1), 'OWNED_DRAWING_EDIT unavailable');
});


test('drawing owner deletion emits the real file-only form contract with a blank password', async () => {
  const source = await readFile(new URL('./drawing-upload.mjs', import.meta.url), 'utf8');
  const template = await readFile(new URL('../../apps/public/templates/post_content.html', import.meta.url), 'utf8');
  const value = /name="file_only" value="([^"]+)"/.exec(template)?.[1];
  assert.equal(value, 'true', 'the native form encodes DeleteForm.file_only as a boolean');
  const start = source.indexOf('  const deleted = await context.request.post(');
  const end = source.indexOf('  assert.equal(deleted.status()', start);
  assert.ok(start > 0 && end > start);
  const run = new Function('context', 'url', 'board', 'origin', 'post',
    `return (async () => { ${source.slice(start, end)} return deleted; })();`);
  const calls = [], response = { status: () => 303 }, origin = new URL('http://127.0.0.1:12345');
  const result = await run({ request: { post: async (...args) => { calls.push(args); return response; } } },
    path => new URL(path, origin).href, 'u12345678', origin, '41');
  assert.equal(result, response);
  assert.deepEqual(calls, [[new URL('/u12345678/delete', origin).href, {
    headers: { Origin: origin.origin }, maxRedirects: 0,
    form: { no: '41', password: '', file_only: value },
  }]]);
});
