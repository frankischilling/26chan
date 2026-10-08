import test from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { installPostingResponseObserver } from './helpers/posting-response.js';
import { sendQuickReply } from '../../apps/public/client/native-quick-reply-transport.js';

const origin = 'http://127.0.0.1:3000';
const url = `${origin}/r9k/imgboard.php`;
const error = 'You have been muted for 2 seconds, because your comment was not original.';
function install(fetch) {
  const window = { fetch };
  runInNewContext(`(${installPostingResponseObserver})({ url, thread: '123', comment: 'duplicate' })`, { window, url });
  return window;
}
function fields(thread = '123', comment = 'duplicate') {
  const body = new FormData(); body.set('resto', thread); body.set('com', comment);
  return { method: 'POST', body };
}

test('observer captures only the exact endpoint, method, thread and draft without issuing extra requests', async () => {
  const calls = [];
  const original = async (...args) => { calls.push(args); return Response.json({ error }); };
  const window = install(original);
  await window.fetch(`${origin}/other/imgboard.php`, fields());
  await window.fetch(url, { ...fields(), method: 'GET' });
  await window.fetch(url, fields('124'));
  await window.fetch(url, fields('123', 'earlier successful reply'));
  assert.notEqual(window.fetch, original);
  const body = fields();
  const response = await window.fetch(url, body);
  const captured = await window.ownedPostingResponse;
  assert.equal(window.fetch, original);
  assert.equal(calls.length, 5);
  assert.equal(calls[4][1], body);
  assert.equal(captured.status, 200);
  assert.equal(captured.type, 'application/json');
  assert.deepEqual(JSON.parse(captured.text), { error });
  assert.deepEqual(await response.json(), { error });
});

test('clone is fully read before the real QR transport consumes and aborts its response', async () => {
  const events = [];
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  let called;
  const started = new Promise(resolve => { called = resolve; });
  const window = install(async (_url, options) => {
    options.signal.addEventListener('abort', () => events.push('transport aborted'));
    const response = Response.json({ error });
    const clone = response.clone.bind(response);
    response.clone = () => {
      const copy = clone();
      return { text: async () => {
        called(); await gate;
        const text = await copy.text(); events.push('clone captured'); return text;
      } };
    };
    return response;
  });
  const sent = sendQuickReply({ board: 'r9k', thread: '123', fields: { com: 'duplicate' }, origin,
    fetcher: (...args) => window.fetch(...args) });
  await started;
  assert.deepEqual(events, []);
  release();
  assert.deepEqual(await sent, { error });
  const captured = await window.ownedPostingResponse;
  assert.deepEqual(JSON.parse(captured.text), { error });
  assert.deepEqual(events, ['clone captured', 'transport aborted']);
});

test('observation failures reject both promises and restore fetch, never becoming a passing response', async () => {
  const failure = new Error('Real response clone unavailable');
  const original = async () => ({ clone: () => ({ text: async () => { throw failure; } }) });
  const window = install(original);
  const observed = assert.rejects(window.ownedPostingResponse, /Real response clone unavailable/);
  await assert.rejects(window.fetch(url, fields()), /Real response clone unavailable/);
  await observed;
  assert.equal(window.fetch, original);
});
