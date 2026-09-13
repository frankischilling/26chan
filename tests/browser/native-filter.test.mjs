import test from 'node:test';
import assert from 'node:assert/strict';
import { Worker } from 'node:worker_threads';
import { once } from 'node:events';
import { FILTER_LIMITS, readNativeFilters, autoWatchBoards, NativeFilterMatcher } from '../../apps/public/static/native-filter.v1.js';

function filter(type, pattern, changes = {}) {
  return { type, pattern, boards: 'demo', active: true, auto: false, ...changes };
}
function matcher(options = {}) {
  const exits = [];
  const instance = new NativeFilterMatcher({ ...options, createWorker() {
    const worker = new Worker(new URL('./helpers/native-filter-worker.mjs', import.meta.url));
    exits.push(once(worker, 'exit'));
    const bridge = { terminate: () => worker.terminate(), postMessage: raw => worker.postMessage(raw) };
    worker.on('message', data => bridge.onmessage?.({ data }));
    worker.on('error', () => bridge.onerror?.({ preventDefault() {} }));
    return bridge;
  } });
  return { instance, async stopped() { await Promise.all(exits); } };
}
async function match(filters, posts, board = 'demo') {
  const owned = matcher();
  try { return await owned.instance.match(filters, board, posts); }
  finally { await owned.stopped(); }
}

test('native filter settings stay bounded and preserve literal pattern text without compiling it', () => {
  assert.deepEqual(readNativeFilters(null), { status: 'ok', filters: [] });
  const row = filter(2, '/(a+)+$/', { auto: true });
  assert.deepEqual(readNativeFilters(JSON.stringify([row])), { status: 'ok', filters: [row] });
  for (const raw of ['', '{}', 'null', '[]'.repeat(FILTER_LIMITS.settings),
    JSON.stringify([filter(3, 'old ID')]), JSON.stringify([filter(2, 'a'.repeat(FILTER_LIMITS.patterns + 1))]),
    JSON.stringify([filter(2, 'x', { active: 'true' })]), JSON.stringify(Array(FILTER_LIMITS.filters + 1).fill(row))]) {
    assert.equal(readNativeFilters(raw).status, 'invalid-settings');
  }
});

test('auto flags choose boards in native order while blank, leading-separator and uppercase scopes retain their meaning', () => {
  const filters = [filter(2, 'x', { auto: true, boards: 'demo, test demo' }),
    filter(2, 'x', { auto: true, boards: 'UPPER' }), filter(2, 'x', { auto: true, boards: '/ignored/' }),
    filter(2, 'x', { auto: true, boards: '' }), filter(2, 'x', { auto: false, boards: 'notselected' }),
    filter(2, '', { auto: true, boards: 'empty' }), filter(2, 'x', { active: false, auto: true, boards: 'inactive' })];
  assert.deepEqual(autoWatchBoards(filters), { status: 'ok', boards: ['demo', 'test', 'UPPER'] });
  const many = Array.from({ length: FILTER_LIMITS.boards + 1 }, (_, n) => filter(2, 'x', { auto: true, boards: `b${n}` }));
  assert.equal(autoWatchBoards(many).status, 'board-limit');
});

test('tripcode, name and poster ID filters use exact strings rather than regular expressions', async () => {
  const rows = [filter(0, '!Trip'), filter(1, '/Alice/i'), filter(4, 'ABC123')];
  assert.deepEqual(await match(rows, [{ no: '1', trip: '!Trip' }, { no: '2', trip: '!trip' },
    { no: '3', name: '/Alice/i' }, { no: '4', name: 'Alice' }, { no: '5', id: 'ABC123' }, { no: '6', id: 'abc123' }]),
  { status: 'ok', matches: [{ id: '1', filter: 0 }, { id: '3', filter: 1 }, { id: '5', filter: 2 }] });
});

test('catalog matching considers non-auto scoped filters but not blank board scopes and keeps the first match', async () => {
  const rows = [filter(5, 'unmatched', { auto: true }), filter(5, 'paper'), filter(5, '/.*/', { boards: '' }), filter(5, 'paper')];
  assert.deepEqual(await match(rows, [{ no: '1', sub: 'Paper craft' }, { no: '2', sub: 'Nothing relevant' }]),
    { status: 'ok', matches: [{ id: '1', filter: 1 }] });
  assert.deepEqual(await match(rows, [{ no: '1', sub: 'paper' }], 'other'), { status: 'ok', matches: [] });
});

test('native word boundaries, AND terms and non-whitespace wildcards retain line-sensitive matching', async () => {
  const rows = [filter(2, 'paper fold*')];
  assert.deepEqual(await match(rows, [{ no: '1', comment: 'Folding PAPER models' },
    { no: '2', comment: 'paper\nfolding' }, { no: '3', comment: 'newspaper folding' },
    { no: '4', comment: 'other line\npaper folded' }]),
  { status: 'ok', matches: [{ id: '1', filter: 0 }, { id: '4', filter: 0 }] });
});

test('quoted patterns remain case-sensitive substrings and preserve the native unescaped pipe', async () => {
  assert.deepEqual(await match([filter(5, '"Paper.fold"')], [{ no: '1', sub: 'a Paper.fold model' },
    { no: '2', sub: 'paper.fold' }, { no: '3', sub: 'PaperXfold' }]),
  { status: 'ok', matches: [{ id: '1', filter: 0 }] });
  assert.deepEqual(await match([filter(5, '"paper|fold"')], [{ no: '1', sub: 'paper' }, { no: '2', sub: 'fold' }, { no: '3', sub: 'other' }]),
    { status: 'ok', matches: [{ id: '1', filter: 0 }, { id: '2', filter: 0 }] });
});

test('native regex syntax supports alternation, case flags, lookaheads and literal-looking hostile text', async () => {
  assert.deepEqual(await match([filter(5, '/^(?!.*plastic).*(paper|fold)/i')],
    [{ no: '1', sub: 'PAPER fold' }, { no: '2', sub: 'plastic paper' }, { no: '3', sub: 'other' }]),
  { status: 'ok', matches: [{ id: '1', filter: 0 }] });
  assert.deepEqual(await match([filter(2, '"<img src=x onerror=alert(1)>"')],
    [{ no: '1', comment: '<img src=x onerror=alert(1)>' }]), { status: 'ok', matches: [{ id: '1', filter: 0 }] });
});

test('empty comments are skipped while missing subject and filename fields retain native undefined coercion', async () => {
  assert.deepEqual(await match([filter(2, '/^$/')], [{ no: '1' }, { no: '2', comment: '' }]), { status: 'ok', matches: [] });
  assert.deepEqual(await match([filter(5, '/^undefined$/')], [{ no: '1' }, { no: '2', sub: '' }]),
    { status: 'ok', matches: [{ id: '1', filter: 0 }] });
  assert.deepEqual(await match([filter(6, '/^undefined$/')], [{ no: '1' }, { no: '2', filename: '' }]),
    { status: 'ok', matches: [{ id: '1', filter: 0 }] });
});

test('invalid regex compilation fails the whole batch instead of applying a partial filter list', async () => {
  assert.deepEqual(await match([filter(5, 'paper'), filter(2, '/[/')], [{ no: '1', sub: 'paper' }]),
    { status: 'invalid-filter', index: 1 });
});

test('requests preserve large IDs and reject oversized, duplicate and malformed fields before worker creation', async () => {
  assert.deepEqual(await match([filter(5, '//')], [{ no: '9007199254740992' }, { no: '9007199254740993' }, { no: '9223372036854775807' }]),
    { status: 'ok', matches: [{ id: '9007199254740992', filter: 0 }, { id: '9007199254740993', filter: 0 }, { id: '9223372036854775807', filter: 0 }] });
  let created = 0;
  const engine = new NativeFilterMatcher({ createWorker() { created++; throw new Error('Must not create'); } });
  for (const posts of [[{ no: 1 }], [{ no: '01' }], [{ no: '9223372036854775808' }], [{ no: '1' }, { no: '1' }],
    [{ no: '1', comment: 'x'.repeat(FILTER_LIMITS.field + 1) }], [{ no: '1', sub: {} }],
    Array.from({ length: FILTER_LIMITS.posts + 1 }, (_, n) => ({ no: String(n + 1) }))]) {
    assert.equal((await engine.match([filter(5, 'x')], 'demo', posts)).status, 'invalid-request');
  }
  assert.equal(created, 0);
});

test('worker responses cannot invent IDs, duplicate or reorder results, or choose inactive or out-of-scope filters', async () => {
  const rows = [filter(5, 'x'), filter(5, 'x', { active: false }), filter(5, 'x', { boards: 'other' })];
  for (const matches of [[{ id: '3', filter: 0 }], [{ id: '1', filter: 0 }, { id: '1', filter: 0 }],
    [{ id: '2', filter: 0 }, { id: '1', filter: 0 }], [{ id: '1', filter: 1 }], [{ id: '1', filter: 2 }],
    [{ id: '1', filter: -1 }], [{ id: 'javascript:alert(1)', filter: 0 }]]) {
    let terminated = false;
    const engine = new NativeFilterMatcher({ createWorker() {
      return { postMessage() { queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({ status: 'ok', matches }) })); },
        terminate() { terminated = true; } };
    } });
    assert.equal((await engine.match(rows, 'demo', [{ no: '1', sub: 'x' }, { no: '2', sub: 'x' }])).status, 'invalid-result');
    assert.equal(terminated, true);
  }
});

test('a real owned worker running a pathological regex is terminated by the deadline while the caller remains responsive', async () => {
  const owned = matcher({ deadline: 150 });
  let ticks = 0;
  const timer = setInterval(() => ticks++, 10);
  try {
    const result = await owned.instance.match([filter(2, '/(a+)+$/')], 'demo', [{ no: '1', comment: 'a'.repeat(48) + '!' }]);
    assert.equal(result.status, 'timeout');
    assert.ok(ticks > 0);
  } finally { clearInterval(timer); await owned.stopped(); }
});

test('cancellation terminates an owned worker and unavailable workers never trigger main-thread evaluation', async () => {
  const owned = matcher();
  const controller = new AbortController();
  const pending = owned.instance.match([filter(2, '/(a+)+$/')], 'demo', [{ no: '1', comment: 'a'.repeat(48) + '!' }], { signal: controller.signal });
  controller.abort();
  assert.equal((await pending).status, 'cancelled');
  await owned.stopped();
  const unavailable = new NativeFilterMatcher({ createWorker() { throw new Error('Denied'); } });
  assert.equal((await unavailable.match([filter(2, '/(a+)+$/')], 'demo', [{ no: '1', comment: 'a'.repeat(48) + '!' }])).status, 'unavailable');
  assert.equal((await unavailable.match([], 'demo', [], { signal: controller.signal })).status, 'cancelled');
});
