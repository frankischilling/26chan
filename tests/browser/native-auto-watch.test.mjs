import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { Worker } from 'node:worker_threads';
import { once } from 'node:events';
import { BLACKLIST_LIMITS, readBlacklist, writeBlacklist, collectAutoWatches, planAutoWatches,
  NativeCatalogTransport, NativeFilterMatcher } from '../../apps/public/static/native-filter.v1.js';
import { WATCH_LIMITS, WatcherRefresh, readWatches, writeWatches } from '../../apps/public/static/thread-watcher-core.v1.js';

const entry = (label = 'Existing', read = '9') => ({ label, read, unread: 2, archived: false, ownReply: false });
const filter = (pattern = 'paper') => ({ type: 5, pattern, active: true, auto: true, boards: 'demo other' });
function matcher(t) {
  const exits = [];
  t.after(async () => { await Promise.all(exits); });
  return new NativeFilterMatcher({ createWorker() {
    const worker = new Worker(new URL('./helpers/native-filter-worker.mjs', import.meta.url));
    exits.push(once(worker, 'exit'));
    const bridge = { terminate: () => worker.terminate(), postMessage: raw => worker.postMessage(raw) };
    worker.on('message', data => bridge.onmessage?.({ data }));
    worker.on('error', () => bridge.onerror?.({ preventDefault() {} }));
    return bridge;
  } });
}
async function server(t, handler) {
  const instance = createServer(handler);
  instance.listen(0, '127.0.0.1');
  await once(instance, 'listening');
  t.after(async () => { instance.closeAllConnections(); await new Promise(resolve => instance.close(resolve)); });
  return `http://127.0.0.1:${instance.address().port}`;
}

test('native blacklist keys remain exact, bounded and fail closed on malformed state', () => {
  const keys = new Set(['9007199254740992-demo', '9007199254740993-demo', '9223372036854775807-other']);
  assert.deepEqual(readBlacklist(writeBlacklist(keys)), { status: 'ok', keys });
  assert.deepEqual(readBlacklist(null), { status: 'ok', keys: new Set() });
  for (const raw of ['[]', 'null', '{', '{"1-demo":true}', '{"1-demo":0}', '{"0-demo":1}',
    '{"1-../demo":1}', '{"9223372036854775808-demo":1}', '{"__proto__":1}',
    ' '.repeat(BLACKLIST_LIMITS.storageChars + 1)]) assert.equal(readBlacklist(raw).status, 'invalid-blacklist');
  const full = new Set(Array.from({ length: BLACKLIST_LIMITS.entries }, (_, i) => `${i + 1}-demo`));
  assert.equal(readBlacklist(writeBlacklist(full)).keys.size, BLACKLIST_LIMITS.entries);
  full.add('5000-demo');
  assert.throws(() => writeBlacklist(full), /blacklist-limit/);
});

test('successful catalogs prune only proven-absent blacklist keys and never overwrite existing watches', () => {
  const current = new Map([['9-demo', entry()]]);
  const blocked = new Set(['1-demo', '2-demo', '3-other', '4-test']);
  const result = planAutoWatches(current, blocked, { status: 'complete', results: [
    { board: 'demo', status: 'ok', present: ['1', '9', '7'], matches: [
      { id: '1', label: 'Blocked' }, { id: '9', label: 'Replacement' }, { id: '7', label: 'New' },
    ] },
    { board: 'other', status: 'http-error' },
  ] });
  assert.deepEqual(result.entries.get('9-demo'), entry());
  assert.deepEqual(result.entries.get('7-demo'), { label: 'New', read: '0', unread: 0, archived: false, ownReply: false });
  assert.equal(result.entries.has('1-demo'), false);
  assert.deepEqual(result.blacklist, new Set(['1-demo', '3-other', '4-test']));
  assert.deepEqual([result.added, result.failed, result.limited], [1, 1, 0]);
  assert.deepEqual(blocked, new Set(['1-demo', '2-demo', '3-other', '4-test']));
  assert.equal(current.size, 1);
});

test('empty successful catalogs prune their own blacklist while failed or unrequested boards remain intact', () => {
  const result = planAutoWatches(new Map(), new Set(['1-demo', '2-other']), {
    status: 'complete', results: [{ board: 'demo', status: 'ok', present: [], matches: [] }],
  });
  assert.deepEqual(result.blacklist, new Set(['2-other']));
  assert.equal(result.entries.size, 0);
});

test('automatic additions respect watch capacity without evicting existing entries or blacklist records', () => {
  const entries = new Map(Array.from({ length: WATCH_LIMITS.entries }, (_, i) => [`${i + 1}-demo`, entry()]));
  const result = planAutoWatches(entries, new Set(['500-other']), { status: 'complete', results: [
    { board: 'demo', status: 'ok', present: ['999'], matches: [{ id: '999', label: 'Over capacity' }] },
  ] });
  assert.equal(result.limited, 1);
  assert.equal(result.added, 0);
  assert.deepEqual(result.entries, entries);
  assert.deepEqual(result.blacklist, new Set(['500-other']));
});

test('invalid or cancelled plans cannot supply arbitrary identities, ordering or labels', () => {
  const valid = { board: 'demo', status: 'ok', present: ['1', '2'], matches: [{ id: '1', label: 'Paper' }] };
  for (const row of [
    { ...valid, board: '../demo' }, { ...valid, present: ['1', '1'] },
    { ...valid, matches: [{ id: '3', label: 'Invented' }] },
    { ...valid, matches: [{ id: '2', label: 'Two' }, { id: '1', label: 'One' }] },
    { ...valid, matches: [{ id: '1', label: 'x'.repeat(46) }] },
    { ...valid, matches: [{ id: '1', label: '\u0000' }] },
  ]) assert.throws(() => planAutoWatches(new Map(), new Set(), { status: 'complete', results: [row] }));
  assert.throws(() => planAutoWatches(new Map(), new Set(), { status: 'cancelled', results: [valid] }));
});

test('worker labels preserve native slicing before entity decoding without returning executable markup', async t => {
  const result = await matcher(t).match([filter('/.*/')], 'demo', [
    { no: '1', sub: 'Paper &amp; fold' },
    { no: '2', sub: '&lt;b&gt;paper&lt;/b&gt;' },
    { no: '3', com: '<b>paper</b><br><br>fold &amp; cut' },
    { no: '4', sub: 'x'.repeat(43) + '&amp;' },
    { no: '5', sub: '<span></span>' },
    { no: '6' },
  ], { labels: true });
  assert.equal(result.status, 'ok');
  assert.deepEqual(result.matches.map(row => row.label), [
    'Paper & fold', '<b>paper</b>', 'paper fold & cut', 'x'.repeat(43) + '&a', '', 'No.6',
  ]);
  const empty = new Map([['5-demo', { ...entry(''), unread: 0 }]]);
  assert.equal(readWatches(writeWatches(empty)).get('5-demo').label, '');
  const comment = await matcher(t).match([{ ...filter('paper'), type: 2 }], 'demo', [
    { no: '7', com: '<b>paper</b><br>fold' },
  ], { labels: true });
  assert.equal(comment.matches[0].label, 'paper fold');
});

test('worker label responses remain bounded and required only for automatic watching', async () => {
  for (const label of [undefined, '\u0000', 'x'.repeat(46)]) {
    let stopped = 0;
    const instance = new NativeFilterMatcher({ createWorker() {
      return { terminate() { stopped++; }, postMessage() {
        this.onmessage({ data: JSON.stringify({ status: 'ok', matches: [{ id: '1', filter: 0, label }] }) });
      } };
    } });
    assert.equal((await instance.match([filter()], 'demo', [{ no: '1', sub: 'paper' }], { labels: true })).status, 'invalid-result');
    assert.equal(stopped, 1);
  }
});

test('owned catalog HTTP and real matching workers produce ordered candidates and preserve failed-board evidence', async t => {
  const origin = await server(t, (request, response) => {
    if (request.url.includes('/other/')) { response.writeHead(503); response.end(); return; }
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end('[{"page":1,"threads":[{"no":9007199254740993,"sub":"paper &amp; fold"}]}]');
  });
  const cycle = await collectAutoWatches({ filters: [filter()], boards: ['demo', 'other'],
    transport: new NativeCatalogTransport({ origin, limits: { staggerMs: 0 } }), matcher: matcher(t) });
  assert.equal(cycle.status, 'complete');
  assert.ok(cycle.bytes > 0);
  assert.deepEqual(cycle.results[0], { board: 'demo', status: 'ok', present: ['9007199254740993'],
    matches: [{ id: '9007199254740993', filter: 0, label: 'paper & fold' }] });
  assert.deepEqual(cycle.results[1], { board: 'other', status: 'http-error' });
});

test('matching failure never supplies an empty success for blacklist pruning', async t => {
  const cycle = await collectAutoWatches({ filters: [filter('/[/')], boards: ['demo'], matcher: matcher(t),
    transport: { refresh: async () => ({ status: 'complete', bytes: 10,
      results: [{ board: 'demo', status: 'ok', posts: [{ no: '1', sub: 'paper' }] }] }) } });
  assert.equal(cycle.results[0].status, 'invalid-filter');
  const plan = planAutoWatches(new Map(), new Set(['2-demo']), cycle);
  assert.deepEqual(plan.blacklist, new Set(['2-demo']));
  assert.equal(plan.entries.size, 0);
});

test('thread refresh accepts only a smaller remaining byte budget and respects the shared cancellation signal', async t => {
  let requests = 0, commits = 0;
  const origin = await server(t, (_request, response) => {
    requests++;
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end('{"posts":[{"no":1,"resto":0}]}');
  });
  const instance = () => new WatcherRefresh({ origin, getEntries: () => new Map([['1-demo', entry('Paper', '1')]]),
    commit: () => { commits++; return true; } });
  for (const bytes of [-1, NaN, 1.5, WATCH_LIMITS.cycleBytes + 1]) {
    assert.equal((await instance().refresh({ bytes })).status, 'invalid-budget');
  }
  assert.equal((await instance().refresh({ bytes: 0 })).results[0].status, 'failed');
  assert.equal(requests, 0);
  assert.equal((await instance().refresh({ bytes: 1 })).results[0].status, 'failed');
  assert.equal(requests, 1);
  assert.equal(commits, 0);
  const controller = new AbortController();
  controller.abort();
  assert.equal((await instance().refresh({ signal: controller.signal })).status, 'cancelled');
  assert.equal(requests, 1);
});

test('shared cancellation settles a hung thread transport even when fetch and body cancellation ignore abort', async () => {
  let cancelled = 0, commits = 0, resolveResponse;
  const refresh = new WatcherRefresh({ origin: 'http://127.0.0.1',
    getEntries: () => new Map([['1-demo', entry('Paper', '1')]]),
    commit: () => { commits++; }, fetcher: () => new Promise(resolve => { resolveResponse = resolve; }) });
  const controller = new AbortController();
  const pending = refresh.refresh({ signal: controller.signal });
  await new Promise(resolve => setTimeout(resolve, 10));
  controller.abort();
  assert.equal((await pending).status, 'cancelled');
  resolveResponse({ body: { cancel() { cancelled++; return new Promise(() => {}); } } });
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(cancelled, 1);
  assert.equal(commits, 0);
});

test('request deadlines bound stalled thread bodies and never wait on an uncooperative cleanup promise', async () => {
  let cancelled = 0;
  const refresh = new WatcherRefresh({ origin: 'http://127.0.0.1',
    getEntries: () => new Map([['1-demo', entry('Paper', '1')]]), commit: () => assert.fail('Unexpected commit'),
    limits: { ...WATCH_LIMITS, requestMs: 25 }, fetcher: async url => ({ url, status: 200, redirected: false,
      headers: new Headers({ 'content-type': 'application/json' }), body: { getReader: () => ({
        read: () => new Promise(() => {}), cancel() { cancelled++; return new Promise(() => {}); },
      }) },
    }),
  });
  const result = await refresh.refresh();
  assert.equal(result.results[0].status, 'failed');
  assert.equal(cancelled, 1);
});

test('a staggered request cannot start after another slot consumes its remaining byte budget', async t => {
  let requests = 0;
  const body = '{"posts":[{"no":1,"resto":0}]}';
  const origin = await server(t, (_request, response) => {
    requests++;
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(body);
  });
  const refresh = new WatcherRefresh({ origin,
    getEntries: () => new Map([['1-demo', entry('One', '1')], ['2-demo', entry('Two', '2')]]),
    commit: () => true, limits: { ...WATCH_LIMITS, staggerMs: 100 } });
  const result = await refresh.refresh({ bytes: Buffer.byteLength(body) });
  assert.equal(result.results[0].status, 'updated');
  assert.equal(result.results[1].status, 'failed');
  assert.equal(requests, 1);
});
