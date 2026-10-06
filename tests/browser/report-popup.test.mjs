import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { mountReportPopup } from '../../apps/public/static/report-popup.v1.js';
import { createReportRegistry, reportURL, REPORT_POPUP_LIMITS } from '../../apps/public/static/thread-watcher-core.v1.js';

const origin = 'https://boards.example';
test('report GET URL accepts only canonical board/post and a bare web origin', () => {
  assert.equal(reportURL(origin, 'demo', '9007199254740993'), `${origin}/demo/imgboard.php?mode=report&no=9007199254740993`);
  for (const id of ['0', '01', '1e2', '9223372036854775808', 12, '1&mode=delete']) assert.equal(reportURL(origin, 'demo', id), null);
  for (const board of ['DEMO', '../demo', 'demo/x', '', 'abcdefghijk', null, { toString: () => 'demo' }]) assert.equal(reportURL(origin, board, '1'), null);
  for (const base of ['null', 'javascript:alert(1)', `${origin}/`, `${origin}/evil`, `https://a@boards.example`]) assert.equal(reportURL(base, 'demo', '1'), null);
});

test('registry requires origin, exact source, complete canonical target message and live original target', () => {
  let live = true, calls = 0;
  const source = { closed: false }, target = {};
  const registry = createReportRegistry({ origin, current: entry => live && entry.target === target,
    complete: () => { calls++; assert.equal(registry.size, 0); } });
  registry.register(source, 'demo', '9007199254740993', target);
  const message = { origin, source, data: 'done-report-9007199254740993-demo' };
  for (const bad of [{ ...message, origin: 'https://evil.example' }, { ...message, source: {} },
    { ...message, source: null }, { ...message, data: {} }, { ...message, data: 'done-report' },
    { ...message, data: 'done-report-9007199254740992-demo' }, { ...message, data: 'done-report-09007199254740993-demo' },
    { ...message, data: 'done-report-9007199254740993-other' }, { ...message, data: `${message.data}-extra` }]) {
    assert.equal(registry.receive(bad), false);
  }
  assert.equal(calls, 0); assert.equal(registry.receive(message), true);
  assert.equal(registry.receive(message), false); assert.equal(calls, 1);
  registry.register(source, 'demo', '9007199254740993', target);
  live = false; assert.equal(registry.receive(message), false); live = true;
  assert.equal(registry.receive(message), false); assert.equal(calls, 1);
});

test('registry bounds live popups, expires stale entries and clears on teardown', () => {
  let now = 10;
  const registry = createReportRegistry({ origin, current: () => true, complete: () => {}, now: () => now });
  const first = { closed: false };
  registry.register(first, 'demo', '1', {});
  for (let i = 0; i < REPORT_POPUP_LIMITS.entries; i++) registry.register({ closed: false }, 'demo', '1', {});
  assert.equal(registry.size, REPORT_POPUP_LIMITS.entries);
  assert.equal(registry.receive({ origin, source: first, data: 'done-report-1-demo' }), false);
  now += REPORT_POPUP_LIMITS.ageMs; assert.equal(registry.size, 0);
  first.closed = false; registry.register(first, 'demo', '1', {});
  first.closed = true; assert.equal(registry.size, 0);
  first.closed = false; registry.register(first, 'demo', '1', {});
  registry.clear(); assert.equal(registry.size, 0);
});

function fixture({ result = 'form', popup = true, opener = true, href = '/demo/thread/1#p2', id = '2', controls = {} } = {}) {
  const messages = [], timers = new Map(), listeners = new Map(), clicks = new Map();
  const doc = { getElementById: name => ({
    'report-popup-context': { dataset: { board: 'demo', post: id, result } },
    'report-popup-return': { getAttribute: () => href },
    'report-popup-close': { addEventListener: (name, fn) => clicks.set(name, fn), removeEventListener: name => clicks.delete(name) },
    ...controls,
  })[name], addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name) };
  const win = { name: popup ? 'report-popup-demo-2-random' : '',
    opener: opener ? { closed: false, location: { origin }, postMessage: (...args) => messages.push(args) } : null,
    location: { origin, assign: url => { win.assigned = url; } }, closedCount: 0,
    close: () => { win.closedCount++; }, setTimeout: (fn, ms) => { timers.set(1, { fn, ms }); return 1; },
    clearTimeout: id => timers.delete(id), addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name) };
  return { doc, win, messages, timers, listeners, clicks, mount: () => mountReportPopup({ window: win, document: doc }) };
}

function categoryControls({ illegalChecked = false, disabled = false, value = '' } = {}) {
  const rule = Object.assign(new EventTarget(), { checked: !illegalChecked });
  const illegal = Object.assign(new EventTarget(), { checked: illegalChecked });
  const category = { disabled, value };
  return {
    rule, illegal, category,
    controls: { 'report-category-rule': rule, 'report-category-illegal': illegal, 'report-category-select': category },
    chooseIllegal(checked) {
      illegal.checked = checked;
      rule.checked = !checked;
      (checked ? illegal : rule).dispatchEvent(new Event('change'));
    },
  };
}

test('category select initializes from the checked radio and follows rule/illegal changes', () => {
  for (const illegalChecked of [false, true]) {
    const c = categoryControls({ illegalChecked, disabled: !illegalChecked });
    const f = fixture({ controls: c.controls }); f.mount();
    assert.equal(c.category.disabled, illegalChecked);
    c.chooseIllegal(true); assert.equal(c.category.disabled, true);
    c.chooseIllegal(false); assert.equal(c.category.disabled, false);
    c.chooseIllegal(true); assert.equal(c.category.disabled, true);
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
  }
});

test('category radio changes preserve both an untouched blank and a prior explicit choice', () => {
  for (const value of ['', '9001']) {
    const c = categoryControls({ value });
    const f = fixture({ controls: c.controls }); f.mount();
    assert.equal(c.category.value, value);
    c.chooseIllegal(true);
    assert.equal(c.category.disabled, true); assert.equal(c.category.value, value);
    c.chooseIllegal(false);
    assert.equal(c.category.disabled, false); assert.equal(c.category.value, value);
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
  }
});

test('missing category controls and illegal-only forms preserve manual cancellation', () => {
  for (const names of [[], ['report-category-illegal'], ['report-category-rule'],
    ['report-category-select'], ['report-category-illegal', 'report-category-select'],
    ['report-category-rule', 'report-category-select']]) {
    const c = categoryControls({ illegalChecked: true, disabled: true });
    const controls = Object.fromEntries(names.map(name => [name, c.controls[name]]));
    const f = fixture({ controls });
    assert.doesNotThrow(() => f.mount());
    c.rule.dispatchEvent(new Event('change'));
    c.illegal.dispatchEvent(new Event('change'));
    if (names.includes('report-category-select')) {
      assert.equal(c.category.disabled, names.includes('report-category-illegal'));
    }
    f.clicks.get('click')(); assert.equal(f.win.closedCount, 1);
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
    assert.doesNotThrow(() => f.listeners.get('pagehide')());
    assert.equal(f.listeners.size, 0); assert.equal(f.clicks.size, 0);
  }
});

test('pagehide removes both category listeners together with close handlers and success timer', () => {
  const c = categoryControls();
  const f = fixture({ result: 'success', controls: c.controls }); f.mount();
  assert.deepEqual(f.messages, [['done-report-2-demo', origin]]);
  assert.equal(f.timers.get(1).ms, 3000);
  c.chooseIllegal(true); assert.equal(c.category.disabled, true);
  f.listeners.get('pagehide')();
  assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0); assert.equal(f.clicks.size, 0);
  c.chooseIllegal(false); assert.equal(c.category.disabled, true, 'rule listener was removed');
  c.category.disabled = false;
  c.chooseIllegal(true); assert.equal(c.category.disabled, false, 'illegal listener was removed');
  assert.equal(f.win.closedCount, 0); assert.equal(f.win.assigned, undefined);
  assert.deepEqual(f.messages, [['done-report-2-demo', origin]]);
});

test('category changes do not bypass opener validation or canonical success targets', () => {
  for (const options of [{ opener: false }, { popup: false }, { id: '3' }]) {
    const c = categoryControls();
    const f = fixture({ ...options, result: 'success', controls: c.controls }); f.mount();
    c.chooseIllegal(true); assert.equal(c.category.disabled, true);
    c.chooseIllegal(false); assert.equal(c.category.disabled, false);
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
  }
  const c = categoryControls();
  const f = fixture({ controls: c.controls }); f.mount();
  Object.defineProperty(f.win.opener, 'location', { get() { throw new Error('foreign'); } });
  c.chooseIllegal(true); c.chooseIllegal(false);
  const event = { key: 'Escape', preventDefault() { this.prevented = true; } };
  f.listeners.get('keydown')(event);
  assert.equal(event.prevented, undefined); assert.equal(f.win.closedCount, 0);
  f.clicks.get('click')(); assert.equal(f.win.assigned, `${origin}/demo/thread/1#p2`);
  assert.equal(f.win.closedCount, 0); assert.deepEqual(f.messages, []);
});

test('only committed success in an accessible popup signals exact origin then closes at 3000ms', () => {
  const f = fixture({ result: 'success' }); f.mount();
  assert.deepEqual(f.messages, [['done-report-2-demo', origin]]);
  assert.equal(f.win.closedCount, 0); assert.equal(f.timers.get(1).ms, 3000);
  f.timers.get(1).fn(); assert.equal(f.win.closedCount, 1);
  f.listeners.get('pagehide')(); assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0); assert.equal(f.clicks.size, 0);
});

test('form, errors and cancellation never signal success or schedule close', () => {
  for (const result of ['form', 'error', '', 'true']) {
    const f = fixture({ result }); f.mount();
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
    f.clicks.get('click')(); assert.equal(f.win.closedCount, 1); assert.deepEqual(f.messages, []);
  }
  const f = fixture(); f.mount();
  const event = { key: 'Escape', preventDefault() { this.prevented = true; } };
  f.listeners.get('keydown')({ ...event, ctrlKey: true }); assert.equal(f.win.closedCount, 0);
  f.listeners.get('keydown')(event); assert.equal(f.win.closedCount, 1); assert.equal(event.prevented, true);
});

test('ordinary, openerless, foreign and closed-opener tabs never auto-close or navigate', () => {
  const fixtures = [fixture({ result: 'success', popup: false }), fixture({ result: 'success', opener: false }), fixture({ result: 'success' }), fixture({ result: 'success' })];
  Object.defineProperty(fixtures[2].win.opener, 'location', { get() { throw new Error('foreign'); } });
  fixtures[3].win.opener.closed = true;
  for (const f of fixtures) {
    f.mount(); assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
    assert.equal(f.win.closedCount, 0); assert.equal(f.win.assigned, undefined);
    f.clicks.get('click')(); assert.equal(f.win.assigned, `${origin}/demo/thread/1#p2`);
    assert.equal(f.win.closedCount, 0);
  }
});

test('Close follows only the canonical same-origin return destination, including unresolved errors', () => {
  for (const href of ['https://evil.example/demo/', '//evil.example/demo/', '/other/', '/demo/thread/01#p2',
    '/demo/thread/1#p3', '/demo/?x=1', 'javascript:alert(1)', 'https://a@boards.example/demo/']) {
    const f = fixture({ popup: false, href }); f.mount(); f.clicks.get('click')(); assert.equal(f.win.assigned, undefined);
  }
  const f = fixture({ popup: false, result: 'error', id: null, href: '/demo/' }); f.mount();
  f.clicks.get('click')(); assert.equal(f.win.assigned, `${origin}/demo/`);
});


test('popup CSP entrypoint has no dependency or dynamic evaluation escape', async () => {
  const source = await readFile(new URL('../../apps/public/static/report-popup.v1.js', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /\bimport\s*(?:[{'"*]|\()/);
  assert.doesNotMatch(source, /\beval\s*\(|new\s+Function\s*\(/);
});


test('Escape never navigates ordinary tabs or popups whose opener became inaccessible', () => {
  const ordinary = fixture({ popup: false }); ordinary.mount();
  const openerless = fixture({ opener: false }); openerless.mount();
  const closed = fixture(); closed.mount(); closed.win.opener.closed = true;
  const foreign = fixture(); foreign.mount();
  Object.defineProperty(foreign.win.opener, 'location', { get() { throw new Error('foreign'); } });
  for (const f of [ordinary, openerless, closed, foreign]) {
    const event = { key: 'Escape', preventDefault() { this.prevented = true; } };
    f.listeners.get('keydown')(event);
    assert.equal(event.prevented, undefined);
    assert.equal(f.win.closedCount, 0); assert.equal(f.win.assigned, undefined);
    assert.equal(f.doc.getElementById('report-popup-return').getAttribute('href'), '/demo/thread/1#p2');
  }
});

test('denied window close is contained for Close, Escape and success timer without navigation', () => {
  const f = fixture({ result: 'success' }); f.mount();
  let attempts = 0;
  f.win.close = () => { attempts++; throw new Error('close denied'); };
  assert.doesNotThrow(() => f.clicks.get('click')());
  assert.doesNotThrow(() => f.listeners.get('keydown')({ key: 'Escape', preventDefault() {} }));
  assert.doesNotThrow(() => f.timers.get(1).fn());
  assert.equal(attempts, 3); assert.equal(f.win.assigned, undefined);
  assert.deepEqual(f.messages, [['done-report-2-demo', origin]]);
  assert.equal(f.doc.getElementById('report-popup-return').getAttribute('href'), '/demo/thread/1#p2');
});


test('targetless error popups preserve manual cancellation without success authority', () => {
  const f = fixture({ result: 'error', id: null }); f.mount();
  assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
  f.clicks.get('click')(); assert.equal(f.win.closedCount, 1);
  f.listeners.get('keydown')({ key: 'Escape', preventDefault() {} });
  assert.equal(f.win.closedCount, 2); assert.equal(f.win.assigned, undefined);
  assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
});

test('popup success requires a valid server target matching the canonical named target', () => {
  for (const id of [null, '', '3', '02', '9223372036854775808']) {
    const f = fixture({ result: 'success', id }); f.mount();
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
    assert.equal(f.win.closedCount, 0); assert.equal(f.win.assigned, undefined);
  }
  for (const name of ['report-popup-other-2-random', 'report-popup-demo-02-random',
    'report-popup-demo-9223372036854775808-random', 'report-popup-demo-2-']) {
    const f = fixture({ result: 'success' }); f.win.name = name; f.mount();
    assert.deepEqual(f.messages, []); assert.equal(f.timers.size, 0);
  }
});
