import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync('apps/staff/static/request-queue.js', 'utf8');
function fixture({ storageThrows = false } = {}) {
  let active = null;
  const timers = new Map(), requests = [], events = new Map(), windowEvents = new Map();
  let sequence = 0;
  class Element {
    constructor(id = '') {
      this.id = id; this.hidden = false; this.value = ''; this.textContent = ''; this.checked = false;
      this.listeners = new Map(); this.classes = new Set(); this.dataset = {};
      this.classList = { add: name => this.classes.add(name), remove: name => this.classes.delete(name), contains: name => this.classes.has(name), toggle: (name, on) => on ? this.classes.add(name) : this.classes.delete(name) };
    }
    addEventListener(name, handler) { this.listeners.set(name, handler); }
    fire(name) { this.listeners.get(name)?.({ preventDefault() {} }); }
    closest() { return null; }
    querySelector() { return null; }
    focus() { active = this; }
    getBoundingClientRect() { return { top: 0, bottom: 50 }; }
    scrollIntoView() {}
  }
  class Form extends Element {
    constructor(row) {
      super(); this.row = row; this.action = 'http://localhost:3001/ban-requests/review';
      this.values = [['id', row.id], ['csrf', 'synthetic']];
    }
    reportValidity() { return true; }
    closest(selector) { return selector === '.ban-request' ? this.row : null; }
  }
  const ids = new Map(['filter-form', 'search-box', 'queue-status', 'queue-undo', 'cfg-cb-dt', 'queue-keybinds', 'queue-preferences', 'reset-btn', 'content', 'title', 'refresh-btn', 'board-menu'].map(id => [id, new Element(id)]));
  ids.get('content').dataset.board = 'g';
  const rows = ['Alpha', 'Beta', 'Gamma'].map((word, index) => {
    const row = new Element(String(index + 1));
    row.fields = { '.post-name': { textContent: word }, '.post-content': { textContent: 'owned' }, '.ban-request-template': { textContent: 'Spam' } };
    row.querySelector = selector => row.fields[selector] || null;
    row.form = new Form(row);
    return row;
  });
  const root = new Element(); root.clientHeight = 900;
  const document = {
    documentElement: root, getElementById: id => ids.get(id),
    addEventListener: (event, callback) => events.set(event, callback),
    querySelectorAll: selector => selector === '#br-list .ban-request' ? rows : [],
  };
  const context = vm.createContext({
    document, Element, HTMLFormElement: Form,
    FormData: class { constructor(form) { this.values = form.values; } [Symbol.iterator]() { return this.values[Symbol.iterator](); } },
    URL, URLSearchParams, AbortController,
    location: { href: 'http://localhost:3001/ban-requests?board=g', reload() {} },
    window: { addEventListener: (event, callback) => windowEvents.set(event, callback) },
    localStorage: { getItem() { if (storageThrows) throw Error(); return null; }, setItem() { if (storageThrows) throw Error(); }, removeItem() { if (storageThrows) throw Error(); } },
    setTimeout: (callback, ms) => { assert.equal(ms, 3000); timers.set(++sequence, callback); return sequence; },
    clearTimeout: id => timers.delete(id),
    fetch: (url, options) => new Promise((resolve, reject) => requests.push({ url, options, resolve, reject })),
  });
  vm.runInContext(source, context);
  return {
    ids, rows, timers, requests, active: () => active,
    key: (key, target = new Element()) => events.get('keydown')({ key, target, preventDefault() {} }),
    submit: (index = 0, decision = 'deny') => events.get('submit')({ target: rows[index].form, submitter: { name: 'decision', value: decision, disabled: false }, preventDefault() {} }),
    run: () => { const callbacks = [...timers.values()]; timers.clear(); for (const callback of callbacks) callback(); },
    hide: () => windowEvents.get('pagehide')(),
  };
}
test('source fields filter loaded requests case-insensitively and reset', () => {
  const f = fixture();
  f.ids.get('search-box').value = 'BETA'; f.ids.get('filter-form').fire('submit');
  assert.deepEqual(f.rows.map(row => row.hidden), [true, false, true]);
  f.ids.get('filter-form').fire('reset');
  assert(f.rows.every(row => !row.hidden));
});
test('focus skips filtered rows, wraps forward and clears before the first row', () => {
  const f = fixture(); f.rows[1].hidden = true;
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[0]);
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[2]);
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[0]);
  f.key('ArrowLeft'); assert(f.rows.every(row => !row.classes.has('focused')));
});
test('pending rows advance focus to the following row rather than wrapping early', () => {
  const f = fixture(); f.key('ArrowRight'); f.key('ArrowRight');
  f.submit(1); assert.equal(f.active(), f.rows[2]);
});
test('review is delayed, repeated submissions do not queue another mutation, and Undo sends nothing', () => {
  const f = fixture(); f.submit(); f.submit();
  assert.equal(f.timers.size, 1); assert.equal(f.requests.length, 0);
  f.ids.get('queue-undo').fire('click'); f.run();
  assert.equal(f.requests.length, 0); assert(!f.rows[0].classes.has('processing'));
});
test('timer sends the immutable submitted values once and prevents a second in-flight review', () => {
  const f = fixture(); f.submit(0, 'accept'); f.rows[0].form.values[0][1] = 'changed'; f.run(); f.submit(1);
  assert.equal(f.requests.length, 1);
  assert.equal(f.requests[0].options.body.get('id'), '1');
  assert.equal(f.requests[0].options.body.get('decision'), 'accept');
  assert.equal(f.requests[0].options.headers.Accept, 'application/json');
});
test('lost response never automatically retries or re-enables stale review controls', async () => {
  const f = fixture(); f.submit(); f.run(); f.requests[0].reject(Error('lost'));
  await new Promise(resolve => setImmediate(resolve));
  f.submit(); f.run(); assert.equal(f.requests.length, 1);
  assert.match(f.ids.get('queue-status').textContent, /Refresh/);
});
test('read refresh is blocked during a pending mutation and navigation clears its timer', () => {
  const f = fixture(); f.submit(); f.ids.get('refresh-btn').fire('click');
  assert.equal(f.requests.length, 0);
  f.hide(); f.run(); assert.equal(f.requests.length, 0);
});
test('unavailable browser storage does not break filtering or review controls', () => {
  const f = fixture({ storageThrows: true });
  f.ids.get('cfg-cb-dt').fire('change'); f.ids.get('queue-keybinds').fire('change');
  f.submit(); f.run(); assert.equal(f.requests.length, 1);
});

test('isolated source filter agrees across names, comments, templates and literal queries', () => {
  const reference = JSON.parse(readFileSync('fixtures/janitor-queue-filter.json', 'utf8'));
  const fields = ['.post-name', '.post-content', '.ban-request-template'];
  const values = ['', 'ALPHA', 'Beta', '<script>owned</script>', 'two words', 'Σigma', 'a\nb', '[literal].*'];
  let count = 0;
  for (const field of fields) for (const text of values) for (const query of values) {
    const f = fixture();
    f.rows[0].fields[field].textContent = text;
    const BR = {};
    const $ = {
      id: id => f.ids.get(id),
      cls: (name, root) => root ? [root.fields['.' + name]].filter(Boolean) : f.rows,
    };
    vm.runInNewContext(reference.functions, { BR, $ });
    BR.applySearch(query);
    const expected = f.rows.map(row => row.classes.has('hidden'));
    BR.clearSearch();
    f.ids.get('search-box').value = query;
    f.ids.get('filter-form').fire('submit');
    assert.deepEqual(f.rows.map(row => row.hidden), expected);
    count++;
  }
  assert.equal(count, 192);
});

test('quick amendments preserve explicit warning and scope through delay and Undo', () => {
  for (const [days, scope] of [['0', ''], ['1', '1'], ['7', '1']]) {
    const f = fixture();
    f.rows[0].form.values.push(['decision', 'accept'], ['amend_tpl', '207'], ['ban_length', days], ['global', scope]);
    f.submit(0, 'accept'); f.run();
    assert.equal(f.requests.length, 1);
    const data = f.requests[0].options.body;
    assert.equal(data.get('amend_tpl'), '207');
    assert.equal(data.get('ban_length'), days);
    assert.equal(data.get('global'), scope);
    const cancelled = fixture();
    cancelled.rows[0].form.values.push(['decision', 'accept'], ['amend_tpl', '207'], ['ban_length', days], ['global', scope]);
    cancelled.submit(0, 'accept'); cancelled.ids.get('queue-undo').fire('click'); cancelled.run();
    assert.equal(cancelled.requests.length, 0);
  }
});

test('W only invokes an enabled warning shortcut on the focused row', () => {
  const f = fixture(); let clicks = 0;
  f.key('w'); assert.equal(clicks, 0);
  f.key('ArrowRight');
  const button = { disabled: false, click() { clicks++; } };
  f.rows[0].fields['button[data-review="Warn"]'] = button;
  f.key('w'); assert.equal(clicks, 1);
  button.disabled = true; f.key('w'); assert.equal(clicks, 1);
  f.ids.get('queue-keybinds').checked = false;
  button.disabled = false; f.key('w'); assert.equal(clicks, 1);
});
