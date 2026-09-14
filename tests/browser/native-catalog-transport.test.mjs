import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { NativeCatalogTransport, catalogApiUrl, CATALOG_TRANSPORT_LIMITS } from '../../apps/public/client/native-catalog-transport.js';
import { WATCH_LIMITS } from '../../apps/public/static/thread-watcher-core.v1.js';

const body = no => `[{"page":1,"threads":[{"no":${no},"com":"<b>paper</b>"}]}]`;
const send = (response, text = body('1')) => { response.writeHead(200, { 'content-type': 'application/json' }); response.end(text); };
async function serverFor(t, handler) {
  const server = createServer(handler);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  return `http://127.0.0.1:${server.address().port}`;
}

test('catalog URLs preserve native board case but cannot change origin or address another route', () => {
  assert.equal(catalogApiUrl('https://board.example', 'DEMO'), 'https://board.example/_watch/DEMO/catalog.json');
  for (const board of ['', '../demo', 'demo/x', 'demo?x', 'demo#x', 'a'.repeat(33), 'demo%2fx']) {
    assert.throws(() => catalogApiUrl('https://board.example', board), /invalid-board/);
  }
  for (const origin of ['file:///tmp', 'https://u:p@board.example', 'https://board.example/path', 'https://board.example/?x', 'https://board.example/#x']) {
    assert.throws(() => catalogApiUrl(origin, 'demo'), /invalid-origin/);
  }
  for (const key of ['responseBytes', 'cycleBytes', 'concurrency', 'staggerMs', 'requestMs', 'cycleMs', 'intervalMs']) {
    assert.equal(CATALOG_TRANSPORT_LIMITS[key], WATCH_LIMITS[key]);
  }
});

test('owned catalog HTTP responses preserve large IDs, input order and credential-free request options', async t => {
  const seen = [];
  const origin = await serverFor(t, (request, response) => {
    seen.push({ url: request.url, cookie: request.headers.cookie, authorization: request.headers.authorization });
    send(response, body(request.url.includes('/demo/') ? '9007199254740992' : '9007199254740993'));
  });
  const transport = new NativeCatalogTransport({ origin, limits: { staggerMs: 0 }, fetcher: (url, options) => {
    assert.equal(options.credentials, 'omit'); assert.equal(options.redirect, 'error');
    assert.equal(options.mode, 'same-origin'); assert.equal(options.cache, 'no-store');
    return fetch(url, options);
  } });
  const result = await transport.refresh(['demo', 'other']);
  assert.equal(result.status, 'complete');
  assert.deepEqual(result.results.map(row => [row.board, row.status, row.posts[0].no]),
    [['demo', 'ok', '9007199254740992'], ['other', 'ok', '9007199254740993']]);
  assert.ok(seen.every(row => row.cookie === undefined && row.authorization === undefined));
});

test('malformed or duplicate board sets and excessive configuration fail before transport starts', async () => {
  let requests = 0;
  const transport = new NativeCatalogTransport({ origin: 'http://127.0.0.1', fetcher: () => { requests++; } });
  for (const boards of [null, ['demo', 'demo'], ['demo/x'], Array.from({ length: 33 }, (_, i) => `b${i}`)]) {
    assert.equal((await transport.refresh(boards)).status, 'invalid-request');
  }
  assert.deepEqual(await transport.refresh([]), { status: 'complete', results: [], bytes: 0 });
  assert.equal(requests, 0);
  for (const limits of [{ concurrency: 0 }, { concurrency: 3 }, { requestMs: 10001 }, { intervalMs: 0 }, { responseBytes: 1.5 }]) {
    assert.throws(() => new NativeCatalogTransport({ origin: 'http://127.0.0.1', limits }), /invalid-limits/);
  }
  assert.equal((await new NativeCatalogTransport({ origin: 'http://127.0.0.1', fetcher: null }).refresh(['demo'])).status, 'unavailable');
});

test('HTTP failures, wrong MIME, malformed catalogs and redirects never become successful board results', async t => {
  let redirected = 0;
  const origin = await serverFor(t, (request, response) => {
    if (request.url === '/healthy-target') { redirected++; send(response); }
    else if (request.url.includes('/missing/')) { response.writeHead(404); response.end(); }
    else if (request.url.includes('/broken/')) { response.writeHead(503); response.end(); }
    else if (request.url.includes('/mime/')) { response.writeHead(200, { 'content-type': 'text/html' }); response.end(body('1')); }
    else if (request.url.includes('/redirect/')) { response.writeHead(302, { location: '/healthy-target' }); response.end(); }
    else send(response, '{"not":"catalog"}');
  });
  const result = await new NativeCatalogTransport({ origin, limits: { staggerMs: 0 } }).refresh(['missing', 'broken', 'mime', 'redirect', 'malformed']);
  assert.equal(result.status, 'complete');
  assert.deepEqual(result.results.map(row => row.status), ['http-error', 'http-error', 'invalid-mime', 'network-error', 'invalid-catalog']);
  assert.equal(redirected, 0);
});

test('declared and streamed response sizes are bounded and invalid UTF-8 is not replaced silently', async t => {
  const origin = await serverFor(t, (request, response) => {
    if (request.url.includes('/declared/')) { response.writeHead(200, { 'content-type': 'application/json', 'content-length': '999' }); response.end('x'); }
    else if (request.url.includes('/streamed/')) { response.writeHead(200, { 'content-type': 'application/json' }); response.write('x'.repeat(25)); response.end('x'.repeat(25)); }
    else { response.writeHead(200, { 'content-type': 'application/json' }); response.end(Buffer.from([0xff])); }
  });
  const result = await new NativeCatalogTransport({ origin, limits: { responseBytes: 40, staggerMs: 0 } }).refresh(['declared', 'streamed', 'encoding']);
  assert.deepEqual(result.results.map(row => row.status), ['response-limit', 'response-limit', 'invalid-encoding']);
});

test('aggregate byte exhaustion settles queued boards without discarding completed board results', async t => {
  const requested = [];
  const text = body('1');
  const origin = await serverFor(t, (request, response) => { requested.push(request.url); send(response, text); });
  const result = await new NativeCatalogTransport({ origin, limits: { concurrency: 1, staggerMs: 0,
    responseBytes: 1024, cycleBytes: Buffer.byteLength(text) + 1 } }).refresh(['first', 'second', 'third']);
  assert.equal(result.status, 'cycle-byte-limit');
  assert.deepEqual(result.results.map(row => row.status), ['ok', 'cycle-byte-limit', 'cycle-byte-limit']);
  assert.equal(requested.length, 2);
  assert.ok(result.bytes <= Buffer.byteLength(text) + 1);
});

test('catalog requests use at most two slots and retain the native 200 ms launch spacing', async t => {
  const starts = [];
  let active = 0, maximum = 0;
  const origin = await serverFor(t, (_request, response) => {
    starts.push(performance.now()); maximum = Math.max(maximum, ++active);
    setTimeout(() => { active--; send(response); }, 300);
  });
  const result = await new NativeCatalogTransport({ origin }).refresh(['first', 'second', 'third']);
  assert.ok(result.results.every(row => row.status === 'ok'));
  assert.equal(maximum, 2);
  assert.ok(starts.slice(1).every((time, i) => time - starts[i] >= 175));
});

test('overlapping requests report busy and completed cycles retain the unchanged refresh interval', async t => {
  let now = 0, requests = 0;
  const origin = await serverFor(t, (_request, response) => { requests++; setTimeout(() => send(response), 30); });
  const transport = new NativeCatalogTransport({ origin, now: () => now });
  const first = transport.refresh(['demo']);
  assert.equal((await transport.refresh(['other'])).status, 'busy');
  assert.equal((await first).results[0].status, 'ok');
  assert.equal((await transport.refresh(['other'])).status, 'cooldown');
  now += WATCH_LIMITS.intervalMs;
  assert.equal((await transport.refresh(['other'])).results[0].status, 'ok');
  assert.equal(requests, 2);
});

test('cancellation settles active and queued requests even if the injected fetch ignores abort', async () => {
  let requests = 0;
  const controller = new AbortController();
  const transport = new NativeCatalogTransport({ origin: 'http://127.0.0.1', limits: { staggerMs: 0 },
    fetcher: () => { requests++; return new Promise(() => {}); } });
  const pending = transport.refresh(['first', 'second', 'third'], { signal: controller.signal });
  await new Promise(resolve => setTimeout(resolve, 20));
  controller.abort();
  const result = await pending;
  assert.equal(result.status, 'cancelled');
  assert.ok(result.results.every(row => row.status === 'cancelled'));
  assert.equal(requests, 2);
});

test('request and cycle deadlines settle hung fetches and do not wait for queued work', async () => {
  const fetcher = () => new Promise(() => {});
  const request = await new NativeCatalogTransport({ origin: 'http://127.0.0.1', fetcher,
    limits: { requestMs: 30, cycleMs: 1000 } }).refresh(['demo']);
  assert.equal(request.status, 'complete');
  assert.equal(request.results[0].status, 'request-timeout');
  const cycle = await new NativeCatalogTransport({ origin: 'http://127.0.0.1', fetcher,
    limits: { concurrency: 1, requestMs: 1000, cycleMs: 30 } }).refresh(['first', 'second', 'third']);
  assert.equal(cycle.status, 'cycle-timeout');
  assert.ok(cycle.results.every(row => row.status === 'cycle-timeout'));
});

test('late responses from a cancelled cycle cannot change its result or a newer cycle', async t => {
  const origin = await serverFor(t, (_request, response) => send(response));
  let resolveOld, now = 0, calls = 0, cancelledBody = false;
  const transport = new NativeCatalogTransport({ origin, now: () => now, fetcher: (url, options) => {
    if (++calls === 1) return new Promise(resolve => { resolveOld = resolve; });
    return fetch(url, options);
  } });
  const pending = transport.refresh(['old']);
  await new Promise(resolve => setTimeout(resolve, 10));
  transport.cancel();
  const old = await pending;
  const snapshot = structuredClone(old);
  now += WATCH_LIMITS.intervalMs;
  const fresh = await transport.refresh(['new']);
  const response = new Response(new ReadableStream({ cancel() { cancelledBody = true; } }), { headers: { 'content-type': 'application/json' } });
  Object.defineProperty(response, 'url', { value: catalogApiUrl(origin, 'old') });
  resolveOld(response);
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.deepEqual(old, snapshot);
  assert.equal(fresh.results[0].status, 'ok');
  assert.equal(cancelledBody, true);
});
