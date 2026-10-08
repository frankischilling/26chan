import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { DELETION_LIMITS, deletionResult, sendNativeDeletion } from '../../apps/public/client/native-post-deletion.js';

const template = await readFile(new URL('../../apps/public/templates/delete_success.html', import.meta.url), 'utf8');
const success = board => template.replace('{{ board }}', board);
const response = (body = success('demo'), options = {}) => new Response(body,
  { headers: { 'content-type': 'text/html; charset=utf-8' }, ...options });
const defaults = { board: 'demo', id: '9223372036854775807', origin: 'https://boards.test' };

test('only the entire current-board fixed server template confirms success', () => {
  assert.equal(deletionResult(success('demo'), 'demo'), true);
  assert.equal(deletionResult(success('a'), 'a'), true);
  for (const body of [success('other'), 'Updating index', "Can't find the post", '<h1>Updating index</h1>',
    success('demo') + '<script>evil()</script>', success('demo').replace('The deletion was completed.', 'Rejected'),
    'x'.repeat(DELETION_LIMITS.responseBytes + 1)]) assert.equal(deletionResult(body, 'demo'), false);
  for (const board of ['../demo', 'a/b', '', 1, 'a'.repeat(11)]) assert.equal(deletionResult(success('demo'), board), false);
});

test('requests preserve exact IDs and source fields, use same-origin cookies and refuse redirects', async () => {
  for (const fileOnly of [false, true]) {
    let calls = 0;
    assert.equal(await sendNativeDeletion({ ...defaults, fileOnly, fetcher: async (url, options) => {
      calls++; assert.equal(url, 'https://boards.test/demo/imgboard.php');
      assert.equal(options.method, 'POST'); assert.equal(options.mode, 'same-origin');
      assert.equal(options.credentials, 'same-origin'); assert.equal(options.redirect, 'error');
      assert.equal(options.cache, 'no-store'); assert.deepEqual(options.headers, { Accept: 'text/html' });
      assert.deepEqual([...options.body.entries()], [['mode', 'usrdel'], [defaults.id, 'delete'],
        ...(fileOnly ? [['onlyimgdel', 'on']] : [])]);
      assert.equal(options.body.has('pwd'), false); assert.equal(options.body.has('password'), false);
      return response();
    } }), true);
    assert.equal(calls, 1);
  }
});

test('invalid targets, origins and bounds are rejected before any network request', async () => {
  let calls = 0;
  const fetcher = async () => { calls++; return response(); };
  for (const change of [{ id: '01' }, { id: '0' }, { id: '9223372036854775808' }, { id: 1 },
    { id: '1&2=delete' }, { board: '../a' }, { board: 'a/'.repeat(3) }, { fileOnly: 'true' },
    { origin: 'https://user:pw@boards.test' }, { origin: 'https://boards.test/path' }, { origin: 'data:text/html,x' },
    { origin: 'https://boards.test/' }, { requestMs: 0 }, { requestMs: DELETION_LIMITS.requestMs + 1 }]) {
    await assert.rejects(sendNativeDeletion({ ...defaults, fetcher, ...change }));
  }
  assert.equal(calls, 0);
});

test('rejections and malformed or wrong-board responses never mark a deletion successful or retry', async () => {
  for (const [body, options, outcome] of [[success('demo'), { status: 403 }, 'rejected'],
    [success('demo'), { status: 500 }, 'unknown'], [success('other'), {}, 'unknown'],
    ['<script>Updating index</script>', {}, 'unknown'], [success('demo'), { status: 201 }, 'unknown'],
    [success('demo'), { headers: { 'content-type': 'application/json' } }, 'unknown']]) {
    let calls = 0;
    await assert.rejects(sendNativeDeletion({ ...defaults, fetcher: async () => { calls++; return response(body, options); } }),
      error => error.deletionOutcome === outcome && /Refresh the page before trying again/.test(error.message));
    assert.equal(calls, 1);
  }
});

test('redirected or alternate response URLs are never accepted', async () => {
  for (const properties of [{ redirected: true }, { url: 'https://foreign.test/demo/imgboard.php' },
    { url: 'https://boards.test/other/imgboard.php' }]) {
    const result = response();
    for (const [key, value] of Object.entries(properties)) Object.defineProperty(result, key, { value });
    await assert.rejects(sendNativeDeletion({ ...defaults, fetcher: async () => result }), /could not be confirmed/);
  }
});

test('oversized, invalid UTF-8 and endless empty streams are bounded and canceled', async () => {
  for (const bytes of [new Uint8Array(DELETION_LIMITS.responseBytes + 1), new Uint8Array([0xff])]) {
    let canceled = false;
    const body = new ReadableStream({ start(controller) { controller.enqueue(bytes); }, cancel() { canceled = true; } });
    await assert.rejects(sendNativeDeletion({ ...defaults, requestMs: 30, fetcher: async () => response(body) }), /could not be confirmed/);
    assert.equal(canceled, true);
  }
  let pulls = 0, canceled = false;
  const body = new ReadableStream({ pull(controller) { pulls++; controller.enqueue(new Uint8Array()); }, cancel() { canceled = true; } });
  await assert.rejects(sendNativeDeletion({ ...defaults, fetcher: async () => response(body) }), /could not be confirmed/);
  assert.ok(pulls <= DELETION_LIMITS.responseBytes + 2); assert.equal(canceled, true);
});

test('declared oversized content is canceled without being read', async () => {
  let reads = 0, canceled = false;
  const body = { getReader() { reads++; throw new Error('unexpected read'); }, cancel() { canceled = true; return Promise.resolve(); } };
  await assert.rejects(sendNativeDeletion({ ...defaults, fetcher: async () => ({ status: 200, body,
    headers: new Headers({ 'content-type': 'text/html', 'content-length': '4097' }) }) }), /could not be confirmed/);
  assert.equal(reads, 0); assert.equal(canceled, true);
});

test('request and body deadlines settle even if an uncooperative fetch or reader ignores abort', async () => {
  let calls = 0, signal;
  await assert.rejects(sendNativeDeletion({ ...defaults, requestMs: 5, fetcher: (_url, options) => {
    calls++; signal = options.signal; return new Promise(() => {});
  } }), /Refresh the page/);
  assert.equal(calls, 1); assert.equal(signal.aborted, true);
  let canceled = false;
  const body = { getReader() { return { read: () => new Promise(() => {}), cancel() { canceled = true; return new Promise(() => {}); } }; } };
  await assert.rejects(sendNativeDeletion({ ...defaults, requestMs: 5, fetcher: async () => ({ status: 200, body,
    headers: new Headers({ 'content-type': 'text/html' }) }) }), /Refresh the page/);
  assert.equal(canceled, true);
});

test('pre-aborted and mid-flight requests never retry, and late responses are canceled', async () => {
  let calls = 0;
  const canceled = new AbortController(); canceled.abort();
  await assert.rejects(sendNativeDeletion({ ...defaults, signal: canceled.signal, fetcher: async () => { calls++; return response(); } }));
  assert.equal(calls, 0);
  let finish, bodyCanceled = false;
  const active = new AbortController();
  const pending = sendNativeDeletion({ ...defaults, signal: active.signal,
    fetcher: () => { calls++; return new Promise(resolve => { finish = resolve; }); } });
  active.abort(); await assert.rejects(pending, /Refresh the page/);
  finish({ body: { cancel() { bodyCanceled = true; return Promise.resolve(); } } });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(bodyCanceled, true); assert.equal(calls, 1);
});
