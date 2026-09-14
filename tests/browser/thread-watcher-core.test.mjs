import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import {
  WATCH_LIMITS, postId, watchKey, splitWatchKey, watchLabel, readWatches, writeWatches,
  sameEntry, orderedWatches, readTrackedReplies, autoRefreshEligible, parseThread,
  quotedPostIds, refreshedEntry, acknowledgedEntry, threadApiUrl, WatcherRefresh,
} from '../../apps/public/static/thread-watcher-core.v1.js';

const entry = (read = '100', extra = {}) => Object.freeze({
  label: 'A paper model', read, unread: 0, archived: false, ownReply: false, ...extra,
});
const wire = (id = '100', replies = ['101'], extra = '') => `{"posts":[{"no":${id},"resto":0${extra}}${replies.map(no => `,{"no":${no},"resto":${id}}`).join('')}]}`;
const fast = { ...WATCH_LIMITS, staggerMs: 0, intervalMs: 0, requestMs: 1000, cycleMs: 5000 };

async function server(t, handler) {
  const app = createServer(handler);
  app.listen(0, '127.0.0.1');
  await once(app, 'listening');
  t.after(() => { app.closeAllConnections(); app.close(); });
  return `http://127.0.0.1:${app.address().port}`;
}

function state(origin, initial, options = {}) {
  const entries = new Map(initial);
  const refresh = new WatcherRefresh({ origin, getEntries: () => entries,
    commit: async (key, expected, next, signal) => {
      if (signal.aborted || !sameEntry(entries.get(key), expected)) return false;
      if (next) entries.set(key, next); else entries.delete(key);
      return true;
    }, limits: fast, ...options });
  return { entries, refresh };
}

test('canonical IDs preserve all positive i64 values without numeric rounding', () => {
  for (const id of ['1', '9007199254740993', '9223372036854775807']) assert.equal(postId(id), id);
  for (const bad of ['', '0', '-1', '+1', '01', '1e3', '1.0', ' 1', '9223372036854775808', 9007199254740992, null, {}]) {
    assert.equal(postId(bad), null, String(bad));
  }
  assert.equal(postId(42), '42');
  assert.equal(postId('0', true), '0');
  assert.equal(watchKey('demo', '9007199254740993'), '9007199254740993-demo');
  for (const key of ['1-../staff', '1-demo/extra', '0-demo', '1-demo?x', '1-DEMO', '__proto__', '9223372036854775808-demo']) {
    assert.equal(splitWatchKey(key), null);
  }
});

test('native tuples round-trip, retaining large read IDs as strings and dead markers as -1', () => {
  const raw = '{"100-demo":["Paper",101,3,1,1],"200-img":["No.200",-1,0],"9007199254740993-demo":["Large","9223372036854775807",0]}';
  const parsed = readWatches(raw);
  assert.equal(parsed.size, 3);
  assert.deepEqual(parsed.get('100-demo'), entry('101', { label: 'Paper', unread: 3, archived: true, ownReply: true }));
  const saved = JSON.parse(writeWatches(parsed));
  assert.equal(saved['200-img'][1], -1);
  assert.equal(saved['9007199254740993-demo'][1], '9223372036854775807');
  assert.deepEqual(readWatches(writeWatches(parsed)), parsed);
});

test('malformed, oversized and invalid local storage is bounded and ignored', () => {
  for (const raw of [null, '', '{', 'null', '[]', '"hello"', 'x'.repeat(WATCH_LIMITS.storageChars + 1)]) {
    assert.equal(readWatches(raw).size, 0);
  }
  for (const tuple of [['x', 1], ['x', 9007199254740992, 0], ['x', 0, -1], ['x', 0, 1.5],
    ['x', 0, 0, 'true'], ['x', 0, 0, 0, {}], ['x'.repeat(46), 0, 0], ['x', 0, 20001]]) {
    assert.equal(readWatches(JSON.stringify({ '100-demo': tuple })).size, 0);
  }
  const many = Object.fromEntries(Array.from({ length: 129 }, (_, i) => [`${i + 1}-demo`, ['x', 0, 0]]));
  assert.equal(readWatches(JSON.stringify(many)).size, 0);
  assert.throws(() => writeWatches(new Map(Object.keys(many).map(key => [key, entry()]))));
  assert.throws(() => writeWatches(new Map([['1-../staff', entry()]])));
  assert.equal(readWatches('{"__proto__":{"polluted":true},"100-demo":["OK",0,0]}').size, 1);
  assert.equal({}.polluted, undefined);
});

test('labels follow subject/comment/fallback priority and 45 UTF-16 units; board sort is stable', () => {
  assert.equal(watchLabel('Subject', 'Comment', '1'), 'Subject');
  assert.equal(watchLabel('', 'Line\nTwo', '1'), 'Line Two');
  assert.equal(watchLabel('', '', '1'), 'No.1');
  assert.equal(watchLabel('x'.repeat(60), '', '1').length, 45);
  assert.equal(watchLabel('\ud83d\ude00'.repeat(30), '', '1').length, 45);
  const rows = new Map([['300-z', entry()], ['200-a', entry()], ['100-a', entry()]]);
  assert.deepEqual(orderedWatches(rows).map(([key]) => key), ['200-a', '100-a', '300-z']);
});

test('tracked own-reply hints accept only bounded native quote keys', () => {
  assert.deepEqual([...readTrackedReplies('{">>100":1,">>9007199254740993":1,">>01":1,"x":1,">>200":true}')], ['100', '9007199254740993']);
  for (const raw of ['{', '[]', 'null', 'x'.repeat(WATCH_LIMITS.trackedChars + 1)]) assert.equal(readTrackedReplies(raw).size, 0);
});

test('catalog and extension first-use automatic refresh eligibility remain distinct', () => {
  assert.equal(autoRefreshEligible(null, true, 100000), false);
  assert.equal(autoRefreshEligible(null, false, 100000), true);
  assert.equal(autoRefreshEligible('40000', true, 100000), true);
  assert.equal(autoRefreshEligible('40001', false, 100000), false);
  assert.equal(autoRefreshEligible('100001', true, 100000), false);
  assert.equal(autoRefreshEligible('NaN', true, 100000), false);
});

test('thread parsing preserves adjacent IDs above 2^53 and validates identity and ordering', () => {
  const parsed = parseThread(wire('9007199254740993', ['9007199254740994', '9007199254740995']), '9007199254740993');
  assert.deepEqual(parsed.posts.map(post => post.id), ['9007199254740993', '9007199254740994', '9007199254740995']);
  const invalid = ['{}', '{"posts":[]}', wire('99'), wire('100', ['102', '101']), wire('100', ['101', '101']),
    '{"posts":[{"no":"100","resto":0}]}', '{"posts":[{"no":1e2,"resto":0}]}',
    '{"posts":[{"no":100,"resto":0},{"no":101,"resto":99}]}', wire('100', [], ',"archived":"yes"'),
    wire('100', [], `,"com":"${'x'.repeat(WATCH_LIMITS.commentChars + 1)}"`)];
  for (const raw of invalid) assert.throws(() => parseThread(raw, '100'));
});

test('quote matching inspects text without creating executable DOM or following links', () => {
  const html = '<img src="https://invalid.example/x" onerror="boom()">'
    + '<a class="quotelink" href="#p100">&gt;&gt;100</a>'
    + '<a class="other quotelink" href="/demo/post/101">&#62;&#x3e;101</a>'
    + '<a href="#p200">&gt;&gt;200</a><a class="quotelink">&gt;&gt;&gt;/other/300</a>';
  assert.deepEqual([...quotedPostIds(html)], ['100', '101']);
});

test('refresh retains unread counts after deletion, marks archives and tracks new own replies only', () => {
  const thread = parseThread(JSON.stringify({ posts: [{ no: 100, resto: 0, archived: 1 },
    { no: 102, resto: 100, com: '<a class="quotelink">&gt;&gt;101</a>' }] }), '100');
  const old = entry('100', { unread: 4 });
  const updated = refreshedEntry(old, thread, new Set(['101']));
  assert.equal(updated.unread, 4);
  assert.equal(updated.archived, true);
  assert.equal(updated.ownReply, true);
  assert.equal(old.archived, false);
  assert.equal(refreshedEntry(entry('102'), thread, new Set(['101'])).ownReply, false);
  assert.equal(refreshedEntry(entry('100', { archived: true }), parseThread(wire(), '100')).archived, true);
  assert.throws(() => refreshedEntry(entry('-1'), thread));
});

test('current-thread acknowledgement advances only; explicit last-read may move backward', () => {
  const before = entry('110', { unread: 5, ownReply: true, archived: true });
  assert.deepEqual(acknowledgedEntry(before, '105'), entry('110', { archived: true }));
  assert.equal(acknowledgedEntry(before, '105', false).read, '105');
  assert.equal(acknowledgedEntry(before, '115').read, '115');
  assert.throws(() => acknowledgedEntry(before, '0'));
});

test('transport URLs are generated from one origin and canonical board/thread keys', () => {
  assert.equal(threadApiUrl('https://board.example', '100-demo'), 'https://board.example/_watch/demo/thread/100.json');
  for (const origin of ['file:///tmp/', 'https://user:secret@board.example', 'https://board.example/staff', 'https://board.example/?x=1']) {
    assert.throws(() => threadApiUrl(origin, '100-demo'));
  }
  assert.throws(() => threadApiUrl('https://board.example', '100-../staff'));
});

test('owned HTTP responses update two boards without credentials and settle every request', async t => {
  let active = 0;
  let maximum = 0;
  const finished = [];
  const origin = await server(t, (request, response) => {
    assert.equal(request.headers.cookie, undefined);
    assert.equal(request.headers.authorization, undefined);
    assert.equal(request.headers.referer, undefined);
    active += 1; maximum = Math.max(maximum, active);
    const id = request.url.match(/\/(\d+)\.json$/)[1];
    setTimeout(() => { active -= 1; finished.push(id); response.writeHead(200, { 'content-type': 'application/json' }); response.end(wire(id, [String(Number(id) + 1)])); }, id === '100' ? 60 : 5);
  });
  const { entries, refresh } = state(origin, [['100-demo', entry('100')], ['200-img', entry('200')], ['300-demo', entry('300')]]);
  const result = await refresh.refresh();
  assert.equal(result.status, 'complete');
  assert.equal(result.results.length, 3);
  assert.equal(finished.length, 3);
  assert.equal(finished.at(-1), '100');
  assert.ok(maximum <= 2);
  for (const value of entries.values()) assert.equal(value.unread, 1);
});

test('404 marks dead, a later refresh removes it, and server failures are not death', async t => {
  let requests = 0;
  const origin = await server(t, (request, response) => { requests++; response.writeHead(request.url.includes('/demo/') ? 404 : 503); response.end(); });
  const { entries, refresh } = state(origin, [['100-demo', entry()], ['200-img', entry('200')]]);
  assert.deepEqual((await refresh.refresh()).results.map(row => row.status), ['dead', 'failed']);
  assert.equal(entries.get('100-demo').read, '-1');
  assert.equal(entries.get('200-img').read, '200');
  assert.deepEqual((await refresh.refresh()).results.map(row => row.status), ['removed', 'failed']);
  assert.equal(entries.has('100-demo'), false);
  assert.equal(requests, 3);
});

test('redirects cannot reach another endpoint; wrong MIME and malformed bodies retain state', async t => {
  let escaped = false;
  const origin = await server(t, (request, response) => {
    if (request.url === '/forbidden') { escaped = true; response.end(); }
    else if (request.url.includes('/100.')) { response.writeHead(302, { location: '/forbidden' }); response.end(); }
    else if (request.url.includes('/200.')) { response.writeHead(200, { 'content-type': 'text/html' }); response.end(wire('200')); }
    else { response.writeHead(200, { 'content-type': 'application/json' }); response.end('{'); }
  });
  const initial = ['100', '200', '300'].map(id => [`${id}-demo`, entry(id)]);
  const { entries, refresh } = state(origin, initial);
  assert.ok((await refresh.refresh()).results.every(row => row.status === 'failed'));
  assert.equal(escaped, false);
  assert.deepEqual([...entries], initial);
});

test('streaming response and aggregate cycle byte limits apply without Content-Length', async t => {
  const origin = await server(t, (_request, response) => { response.writeHead(200, { 'content-type': 'application/json' }); response.write(' '.repeat(200)); response.end(wire()); });
  for (const limits of [{ ...fast, responseBytes: 128 }, { ...fast, cycleBytes: 128 }]) {
    const { entries, refresh } = state(origin, [['100-demo', entry()]], { limits });
    assert.equal((await refresh.refresh()).results[0].status, 'failed');
    assert.equal(entries.get('100-demo').unread, 0);
  }
});

test('declared oversized bodies and invalid UTF-8 fail without modifying state', async t => {
  const origin = await server(t, (request, response) => {
    if (request.url.includes('/100.')) { response.writeHead(200, { 'content-type': 'application/json', 'content-length': '99999999' }); response.flushHeaders(); }
    else { response.writeHead(200, { 'content-type': 'application/json' }); response.end(Buffer.from([0xff, 0xfe])); }
  });
  const { refresh } = state(origin, [['100-demo', entry()], ['200-demo', entry('200')]]);
  assert.ok((await refresh.refresh()).results.every(row => row.status === 'failed'));
});

test('unwatch, read acknowledgement and cross-tab edits cannot be overwritten by delayed responses', async t => {
  let resolveStarted;
  const started = new Promise(resolve => { resolveStarted = resolve; });
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const origin = await server(t, async (_request, response) => { resolveStarted(); await gate; response.writeHead(200, { 'content-type': 'application/json' }); response.end(wire()); });
  const { entries, refresh } = state(origin, [['100-demo', entry()]]);
  const pending = refresh.refresh();
  await started;
  entries.set('100-demo', acknowledgedEntry(entry(), '101'));
  release();
  assert.equal((await pending).results[0].status, 'stale');
  assert.equal(entries.get('100-demo').read, '101');
  const next = refresh.refresh();
  entries.delete('100-demo');
  assert.equal((await next).results[0].status, 'stale');
  assert.equal(entries.size, 0);
});

test('cancellation and newer generations do not resurrect watches even when transport ignores abort', async () => {
  const responses = [];
  const fetcher = (url, options) => new Promise(resolve => responses.push(() => {
    const response = new Response(wire(), { headers: { 'content-type': 'application/json' } });
    Object.defineProperty(response, 'url', { value: url });
    assert.equal(options.credentials, 'omit');
    assert.equal(options.redirect, 'error');
    resolve(response);
  }));
  const { entries, refresh } = state('http://127.0.0.1', [['100-demo', entry()]], { fetcher });
  const first = refresh.refresh();
  await new Promise(resolve => setImmediate(resolve));
  const second = refresh.refresh();
  await new Promise(resolve => setImmediate(resolve));
  responses[1]();
  assert.equal((await second).status, 'complete');
  entries.delete('100-demo');
  responses[0]();
  assert.equal((await first).status, 'cancelled');
  assert.equal(entries.size, 0);
});

test('request deadlines settle hung owned responses and local frequency limits survive cancellation', async t => {
  const origin = await server(t, (_request, response) => { response.writeHead(200, { 'content-type': 'application/json' }); response.flushHeaders(); });
  const { entries, refresh } = state(origin, [['100-demo', entry()]], { limits: { ...fast, requestMs: 40, intervalMs: 60000 } });
  const result = await refresh.refresh();
  assert.equal(result.results[0].status, 'failed');
  assert.equal(entries.get('100-demo').read, '100');
  refresh.cancel();
  assert.equal((await refresh.refresh()).status, 'cooldown');
});

test('cycle timeout cancels queued work and returns a terminal result for every entry', async t => {
  const origin = await server(t, (_request, response) => { response.writeHead(200, { 'content-type': 'application/json' }); response.flushHeaders(); });
  const { refresh } = state(origin, ['100', '200', '300'].map(id => [`${id}-demo`, entry(id)]),
    { limits: { ...fast, cycleMs: 40, staggerMs: 200 } });
  const result = await refresh.refresh();
  assert.equal(result.status, 'cancelled');
  assert.equal(result.results.length, 3);
  assert.ok(result.results.every(row => row.status === 'cancelled'));
});
