import test from 'node:test';
import assert from 'node:assert/strict';
import {
  THREAD_STATS_LIMITS,
  NativeThreadStatsTransport,
  parseThreadStats,
  threadStatsApiUrl,
  threadStatsContext,
} from '../../apps/public/static/native-thread-stats.v1.js';

const context = { origin: 'https://boards.test', board: 'demo', thread: '123' };
const snapshot = {
  version: 1,
  board: 'demo',
  thread: '123',
  replies: 12,
  images: 5,
  sticky: false,
  closed: false,
  archived: false,
  bump_limited: false,
  image_limited: false,
  page: 2,
};

function response(raw, { url = threadStatsApiUrl(context), status = 200, redirected = false,
  contentType = 'application/json; charset=utf-8', declared = null, chunks = null } = {}) {
  const encoder = new TextEncoder();
  const encoded = raw instanceof Uint8Array ? raw : encoder.encode(raw);
  const parts = chunks ?? [encoded];
  const headers = new Headers();
  if (contentType !== null) headers.set('content-type', contentType);
  if (declared !== null) headers.set('content-length', String(declared));
  return {
    url, status, redirected, headers,
    body: new ReadableStream({
      start(controller) {
        for (const part of parts) controller.enqueue(part);
        controller.close();
      },
    }),
  };
}

test('thread stats context and URL accept only canonical same-origin board/thread identities', () => {
  assert.deepEqual(threadStatsContext(context), context);
  assert.equal(threadStatsApiUrl(context), 'https://boards.test/_watch/demo/thread/123/stats');
  for (const invalid of [
    { ...context, origin: 'https://boards.test/path' },
    { ...context, origin: 'javascript:alert(1)' },
    { ...context, board: 'Demo' },
    { ...context, board: 'x'.repeat(11) },
    { ...context, thread: null },
    { ...context, thread: '0123' },
    { ...context, thread: '0' },
    { ...context, thread: '9223372036854775808' },
  ]) assert.throws(() => threadStatsContext(invalid), /invalid-thread-stats-context/);
});

test('parser requires the exact typed v1 object and validates optional poster counts', () => {
  assert.deepEqual(parseThreadStats(JSON.stringify(snapshot), context), snapshot);
  const counted = { ...snapshot, unique_ips: 7 };
  assert.deepEqual(parseThreadStats(JSON.stringify(counted), context), counted);
  const archived = { ...snapshot, archived: true, closed: true, page: null };
  assert.deepEqual(parseThreadStats(JSON.stringify(archived), context), archived);
  const invalid = [
    ...[0, -1, 14, 1.5, '7', null].map(unique_ips => ({ ...snapshot, unique_ips })),
    { ...archived, unique_ips: 1 },
    { ...snapshot, posters: 7 },
    { ...snapshot, thread: 123 },
    { ...snapshot, board: 'other' },
    { ...snapshot, replies: 1001 },
    { ...snapshot, images: 13 },
    { ...snapshot, bump_limited: 1 },
    { ...snapshot, page: null },
    { ...snapshot, archived: true },
    { ...snapshot, page: 0 },
    { ...snapshot, page: 1001 },
  ];
  for (const value of invalid) assert.throws(() => parseThreadStats(JSON.stringify(value), context), /invalid-thread-stats/);
  assert.throws(() => parseThreadStats('{"version":1', context), /invalid-thread-stats/);
  assert.throws(() => parseThreadStats('x'.repeat(THREAD_STATS_LIMITS.bytes + 1), context), /invalid-thread-stats/);
});

test('transport makes one strict same-origin bounded request and validates a streamed response', async () => {
  const calls = [];
  const raw = JSON.stringify(snapshot), bytes = new TextEncoder().encode(raw);
  const transport = new NativeThreadStatsTransport({ ...context, fetcher: async (url, options) => {
    calls.push({ url, options });
    return response(raw, { declared: bytes.length,
      chunks: [bytes.slice(0, 5), bytes.slice(5, 17), bytes.slice(17)] });
  } });
  const result = await transport.load();
  assert.equal(result.status, 'ok');
  assert.deepEqual(result.snapshot, snapshot);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, threadStatsApiUrl(context));
  assert.deepEqual({ method: calls[0].options.method, credentials: calls[0].options.credentials,
    mode: calls[0].options.mode, redirect: calls[0].options.redirect, cache: calls[0].options.cache,
    accept: calls[0].options.headers.Accept },
  { method: 'GET', credentials: 'omit', mode: 'same-origin', redirect: 'error', cache: 'no-store', accept: 'application/json' });
  assert.equal(transport.active, null);
});

test('transport is single-flight and its deadline resolves even when a fetcher ignores abort', async () => {
  let release;
  const held = new Promise(resolve => { release = resolve; });
  const first = new NativeThreadStatsTransport({ ...context, fetcher: () => held });
  const pending = first.load();
  assert.deepEqual(await first.load(), { status: 'busy' });
  release(response(JSON.stringify(snapshot)));
  assert.equal((await pending).status, 'ok');

  const stalled = new NativeThreadStatsTransport({ ...context, fetcher: () => new Promise(() => {}), limits: { requestMs: 20 } });
  const started = Date.now();
  assert.deepEqual(await stalled.load(), { status: 'timeout' });
  assert.ok(Date.now() - started < 1000);
  assert.equal(stalled.active, null);
});

test('transport rejects redirects, foreign URLs, MIME mismatches, oversized and partial bodies', async () => {
  const cases = [
    response(JSON.stringify(snapshot), { redirected: true }),
    response(JSON.stringify(snapshot), { url: 'https://foreign.test/_watch/demo/thread/123/stats' }),
    response(JSON.stringify(snapshot), { contentType: 'text/html' }),
    response(JSON.stringify(snapshot), { declared: THREAD_STATS_LIMITS.bytes + 1 }),
    response(new Uint8Array(THREAD_STATS_LIMITS.bytes + 1)),
    response(new Uint8Array([0xc3, 0x28])),
    response('{"version":1'),
    response(JSON.stringify({ ...snapshot, unique_ips: 0 })),
    response(JSON.stringify({ ...snapshot, posters: 3 })),
  ];
  const expected = ['invalid-response', 'invalid-response', 'invalid-response', 'response-limit',
    'response-limit', 'invalid-encoding', 'invalid-snapshot', 'invalid-snapshot', 'invalid-snapshot'];
  for (let i = 0; i < cases.length; i++) {
    const transport = new NativeThreadStatsTransport({ ...context, fetcher: async () => cases[i] });
    assert.equal((await transport.load()).status, expected[i]);
  }
});

test('external cancellation settles the request and revokes the active slot', async () => {
  const transport = new NativeThreadStatsTransport({ ...context, fetcher: () => new Promise(() => {}) });
  const controller = new AbortController();
  const pending = transport.load({ signal: controller.signal });
  controller.abort();
  assert.deepEqual(await pending, { status: 'cancelled' });
  assert.equal(transport.active, null);
});

test('empty-chunk streams terminate within finite work and a healthy response can follow', async () => {
  let reads = 0, cancelled = 0, healthy = false;
  const transport = new NativeThreadStatsTransport({ ...context, fetcher: async url => healthy
    ? response(JSON.stringify(snapshot))
    : { url, status: 200, headers: new Headers({ 'content-type': 'application/json' }),
      body: { getReader() { return {
        async read() { reads++; return { done: false, value: new Uint8Array() }; },
        async cancel() { cancelled++; },
      }; } } } });
  assert.equal((await transport.load()).status, 'response-limit');
  assert.equal(reads, 4097);
  assert.equal(cancelled, 1);
  assert.equal(transport.active, null);
  healthy = true;
  assert.deepEqual((await transport.load()).snapshot, snapshot);
});
