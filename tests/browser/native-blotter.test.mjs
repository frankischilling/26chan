import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { BLOTTER_STORAGE_KEY, blotterTimestamp, mountNativeBlotter } from '../../apps/public/client/native-blotter.js';

function fixture(timestamp = '200', initial = null, failure = null, absent = false) {
  const values = new Map(initial === null ? [] : [[BLOTTER_STORAGE_KEY, initial]]);
  const counts = { read: 0, write: 0, remove: 0 };
  const store = {
    getItem(key) { counts.read++; if (failure === 'read') throw Error(); return values.get(key) ?? null; },
    setItem(key, value) { counts.write++; if (failure === 'write') throw Error(); values.set(key, value); },
    removeItem(key) { counts.remove++; if (failure === 'remove') throw Error(); values.delete(key); },
  };
  const button = new EventTarget();
  button.hidden = false;
  const attributes = new Map([['data-utc', timestamp]]);
  button.getAttribute = key => attributes.get(key);
  button.setAttribute = (key, value) => attributes.set(key, value);
  const messages = { hidden: false }, all = { hidden: false };
  const view = { get localStorage() { if (failure === 'access') throw Error(); return store; } };
  const document = { defaultView: view, getElementById: id => absent ? null : ({ toggleBlotter: button, 'blotter-msgs': messages, 'blotter-all': all })[id] };
  return { document, button, messages, all, values, counts, attributes,
    click() { const event = new Event('click', { cancelable: true }); button.dispatchEvent(event); assert.equal(event.defaultPrevented, true); } };
}

test('hide, reload and show use the source storage key and labels', () => {
  const h = fixture(); mountNativeBlotter(h.document);
  assert.equal(h.button.hidden, false); assert.equal(h.button.textContent, 'Hide');
  h.click(); assert.equal(h.messages.hidden, true); assert.equal(h.all.hidden, true);
  assert.equal(h.values.get('4chan-blotter'), '200'); assert.equal(h.button.textContent, 'Show Blotter');
  assert.equal(h.attributes.get('aria-expanded'), 'false');
  const reloaded = fixture('200', h.values.get(BLOTTER_STORAGE_KEY)); mountNativeBlotter(reloaded.document);
  assert.equal(reloaded.messages.hidden, true); reloaded.click();
  assert.equal(reloaded.messages.hidden, false); assert.equal(reloaded.all.hidden, false);
  assert.equal(reloaded.button.textContent, 'Hide'); assert.equal(reloaded.attributes.get('aria-expanded'), 'true');
  assert.equal(reloaded.values.has(BLOTTER_STORAGE_KEY), false);
});
for (const [current, seen, hidden] of [['200', '200', true], ['199', '200', true], ['201', '200', false]]) {
  test(`timestamp ${current} after hiding ${seen}: hidden=${hidden}`, () => {
    const h = fixture(current, seen); mountNativeBlotter(h.document); assert.equal(h.messages.hidden, hidden);
  });
}
for (const invalid of [null, '', 'NaN', 'Infinity', '-1', '1e3', '1.5', ' 200', '0200', '9'.repeat(100000), '8640000000001', {}, 200]) {
  test(`malformed storage (${String(invalid).slice(0, 20)}) keeps messages visible`, () => {
    assert.equal(blotterTimestamp(invalid), null);
    const h = fixture('200', invalid); mountNativeBlotter(h.document); assert.equal(h.messages.hidden, false);
  });
}
for (const failure of ['access', 'read', 'write', 'remove']) test(`storage ${failure} failure leaves controls usable`, () => {
  const h = fixture('200', null, failure); mountNativeBlotter(h.document);
  h.click(); assert.equal(h.messages.hidden, true); h.click(); assert.equal(h.messages.hidden, false);
});
test('disabled or empty markup has no storage access or event listener', () => {
  const h = fixture('200', null, null, true); assert.equal(mountNativeBlotter(h.document), null);
  assert.deepEqual(h.counts, { read: 0, write: 0, remove: 0 });
});
test('mount is idempotent and destroy permits one remount', () => {
  const h = fixture(); const controller = mountNativeBlotter(h.document);
  assert.equal(mountNativeBlotter(h.document), controller); h.click(); assert.equal(h.messages.hidden, true);
  assert.equal(h.counts.write, 1); controller.destroy(); assert.equal(h.button.hidden, false);
  assert.equal(h.messages.hidden, false); mountNativeBlotter(h.document); h.click(); assert.equal(h.messages.hidden, false);
});
test('invalid server timestamp does not install controls', () => {
  const h = fixture('bad'); assert.equal(mountNativeBlotter(h.document), null); assert.equal(h.button.hidden, false);
});
test('template keeps preview in the posting branch and escapes content on both pages', async () => {
  const root = new URL('../../apps/public/', import.meta.url);
  const board = await readFile(new URL('templates/board.html', root), 'utf8');
  const preview = await readFile(new URL('templates/blotter_preview.html', root), 'utf8');
  const page = await readFile(new URL('templates/blotter.html', root), 'utf8');
  assert.match(board, /{% include "blotter_preview.html" %}\n<\/form>\n{% endif %}\n<hr>/);
  assert.match(preview, /board.show_blotter && !blotter.is_empty\(\)/);
  for (const template of [preview, page]) {
    assert.match(template, /{{ message.content }}/); assert.doesNotMatch(template, /\|\s*safe|onclick|onerror/);
  }
  assert.match(page, /id="msg-{{ message.id }}"/); assert.match(page, /\/blotter\?offset={{ offset }}/);
  assert.match(preview, /href="\/blotter" target="_blank" rel="noopener"/);
  assert.match(preview, /\[<a id="toggleBlotter" href="#"/);
  assert.match(preview, /<thead><tr><td colspan="2"><hr class="aboveMidAd"><\/td><\/tr><\/thead>/);
});
test('blotter mounts before the existing sole MainInit dispatch without new event authority', async () => {
  const watcher = await readFile(new URL('../../apps/public/static/thread-watcher.v1.js', import.meta.url), 'utf8');
  assert.equal((watcher.match(/dispatchSourceEvent\(document, '4chanMainInit'\)/g) || []).length, 1);
  assert.match(watcher, /if \(!catalog && document.querySelector\('\.board'\)\) \{\s*mountNativeBlotter\(document\);\s*dispatchSourceEvent\(document, '4chanMainInit'\)/);
});

test('pinned source confirms dismissal, labels and newer-message visibility', async () => {
  const { default: vm } = await import('node:vm');
  const { createHash } = await import('node:crypto');
  const source = JSON.parse(await readFile(new URL('../fixtures/native-blotter-source.json', import.meta.url), 'utf8'));
  assert.equal(source.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  assert.equal(Buffer.byteLength(source.text), source.byte_end - source.byte_start);
  assert.equal(createHash('sha256').update(source.text).digest('hex'), source.sha256);
  for (const seen of [null, '199', '200', '201']) {
    const native = fixture('200', seen); mountNativeBlotter(native.document);
    const values = new Map(seen === null ? [] : [['4chan-blotter', seen]]);
    const all = { style: { display: '' } }, messages = { style: { display: '' } };
    const button = { textContent: 'Hide', nextElementSibling: all, addEventListener() {}, getAttribute: () => '200' };
    const sandbox = { document: { getElementById: id => ({ toggleBlotter: button, 'blotter-msgs': messages })[id] },
      localStorage: { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) } };
    vm.createContext(sandbox); vm.runInContext(source.text, sandbox, { timeout: 100 }); sandbox.initBlotter();
    assert.equal(native.messages.hidden, messages.style.display === 'none');
    assert.equal(native.all.hidden, all.style.display === 'none'); assert.equal(native.button.textContent, button.textContent);
    assert.equal(native.values.get('4chan-blotter'), values.get('4chan-blotter'));
    native.click(); sandbox.toggleBlotter({ preventDefault() {} });
    assert.equal(native.messages.hidden, messages.style.display === 'none');
    assert.equal(native.button.textContent, button.textContent); assert.equal(native.values.get('4chan-blotter'), values.get('4chan-blotter'));
  }
});

test('all six pinned theme excerpts agree on preview geometry', async () => {
  const { createHash } = await import('node:crypto');
  const reference = JSON.parse(await readFile(new URL('../fixtures/native-blotter-geometry.json', import.meta.url), 'utf8'));
  assert.equal(reference.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  assert.deepEqual(reference.themes.map(row => row.theme).sort(), ['burichan', 'futaba', 'photon', 'tomorrow', 'yotsuba', 'yotsuba-b']);
  for (const row of reference.themes) {
    assert.equal(createHash('sha256').update(row.rules).digest('hex'), row.rules_sha256);
    assert.match(row.rules, /#blotter\s*\{\s*width: 468px;\s*margin: auto;/);
    assert.match(row.rules, /#blotter td\s*\{\s*vertical-align: top;\s*font-size: 11px;/);
    assert.match(row.rules, /\.blotter-date\s*\{\s*width: 50px;\s*text-align: center;/);
    assert.match(row.rules, /#blotter tfoot\s*\{\s*text-align: right;/);
  }
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8');
  assert.match(css, /#blotter \{ width: 468px; margin: auto;/);
  assert.match(css, /#blotter td \{[^}]*font-size: 11px;/);
  assert.match(css, /#blotter \.blotter-date \{ width: 50px; text-align: center;/);
  const preview = await readFile(new URL('../../apps/public/templates/blotter_preview.html', import.meta.url), 'utf8');
  assert.doesNotMatch(preview, /\bhidden\b/);
});
