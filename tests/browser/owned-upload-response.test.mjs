import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { chromium } from '@playwright/test';
import { sendNativeDeletion } from '../../apps/public/client/native-post-deletion.js';
import { assertOwnedThreadResponse, observeOwnedDeletionResponse, observeOwnedUploadResponse, ownedDeletionResponse, ownedUploadResponse } from './owned-upload-response.mjs';

const response = (status, type, json) => ({ status: () => status, headers: () => ({ 'content-type': type }), json });

test('owner-thread failures expose only bounded HTTP status, category and stage without reading a body', () => {
  for (const [status, mime, type] of [[422, 'text/html; private=value', 'html'], [429, 'text/plain', 'plain'],
    [403, 'application/json', 'json'], [500, 'private-content-type', 'other']]) {
    const diagnostics = [];
    assert.throws(() => assertOwnedThreadResponse(response(status, mime, () => { throw new Error('body must not be read'); }),
      value => diagnostics.push(value)), /^Error: Owned thread response was not a creation redirect\.$/);
    assert.deepEqual(diagnostics, [`OWNED_UPLOAD_RESPONSE status=${status} type=${type} stage=owner-thread failure=http`]);
  }
});

test('owner-thread creation succeeds silently and invalid statuses never reach diagnostics', () => {
  const diagnostics = [];
  assertOwnedThreadResponse({ status: () => 303 }, value => diagnostics.push(value));
  for (const status of [99, 600, 200.5, '422 private-value', NaN]) {
    assert.throws(() => assertOwnedThreadResponse({ status: () => status }, value => diagnostics.push(value)),
      /^Error: Invalid owned response status\.$/);
  }
  assert.deepEqual(diagnostics, []);
});

test('both successful workflows retain the exact parsed response', async () => {
  for (const stage of ['upload', 'post']) {
    const result = { opaque: 'private-value' }, diagnostics = [];
    assert.deepEqual(await ownedUploadResponse(response(200, 'application/json', async () => result), stage,
      message => diagnostics.push(message)), { status: 200, result });
    assert.deepEqual(diagnostics, []);
  }
});

test('body and JSON failures retain numeric status and fixed classifications only', async () => {
  for (const stage of ['upload', 'post']) {
    for (const [error, failure] of [[new Error('private-capability in browser URL'), 'body'], [new SyntaxError('private response bytes'), 'json']]) {
      const diagnostics = [];
      await assert.rejects(ownedUploadResponse(response(200, 'application/json', async () => { throw error; }), stage,
        message => diagnostics.push(message)), /^Error: Owned upload response could not be read\.$/);
      assert.deepEqual(diagnostics, [`OWNED_UPLOAD_RESPONSE status=200 type=json stage=${stage} failure=${failure}`]);
    }
  }
});

test('unexpected status and type remain visible without erasing the failed read', async () => {
  const diagnostics = [];
  await assert.rejects(ownedUploadResponse(response(429, 'text/plain; private=value', async () => { throw new SyntaxError('private body'); }),
    'upload', message => diagnostics.push(message)));
  assert.deepEqual(diagnostics, [
    'OWNED_UPLOAD_RESPONSE status=429 type=plain stage=upload failure=http',
    'OWNED_UPLOAD_RESPONSE status=429 type=plain stage=upload failure=json',
  ]);
});

test('unknown response stages fail before accessing response data', async () => {
  await assert.rejects(ownedUploadResponse(null, 'private-value'), /^Error: Unknown owned response stage\.$/);
  await assert.rejects(observeOwnedUploadResponse(null, 'private-value', 'private-value'),
    /^Error: Unknown owned response stage\.$/);
});

test('browser capture retains the real upload and posting bodies after transport cleanup', { timeout: 30000 }, async () => {
  const [transport, core] = await Promise.all([
    readFile(new URL('../../apps/public/client/native-quick-reply-transport.js', import.meta.url)),
    readFile(new URL('../../apps/public/static/thread-watcher-core.v1.js', import.meta.url)),
  ]);
  const receipt = { upload_id: '1'.repeat(32), upload_capability: '2'.repeat(64), resto: '10', state: 'queued' };
  const counts = { upload: 0, post: 0 };
  const server = createServer((request, response) => {
    request.resume();
    if (request.method === 'POST' && request.url === '/demo/upload') {
      counts.upload++;
      response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(receipt));
    } else if (request.method === 'POST' && request.url === '/demo/imgboard.php') {
      counts.post++;
      response.writeHead(200, { 'content-type': 'application/json' }).end('{"tid":10,"pid":11}');
    } else if (request.url === '/client/native-quick-reply-transport.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(transport);
    } else if (request.url === '/static/thread-watcher-core.v1.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(core);
    } else {
      response.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><title>Owned transport response</title>');
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  let browser;
  try {
    browser = await chromium.launch({ timeout: 15000 });
    const page = await browser.newPage();
    await page.goto(origin);
    const diagnostics = [];
    const uploaded = await observeOwnedUploadResponse(page, `${origin}/demo/upload`, 'upload', value => diagnostics.push(value));
    const uploadResult = await page.evaluate(async () => {
      const { uploadQuickReplyFile } = await import('/client/native-quick-reply-transport.js');
      return uploadQuickReplyFile({ board: 'demo', thread: '10', file: new File(['x'], 'owned.png') });
    });
    assert.deepEqual(uploadResult, receipt);
    assert.deepEqual(await uploaded(), { status: 200, result: receipt });
    const posted = await observeOwnedUploadResponse(page, `${origin}/demo/imgboard.php`, 'post', value => diagnostics.push(value));
    const postResult = await page.evaluate(async () => {
      const { sendQuickReply } = await import('/client/native-quick-reply-transport.js');
      return sendQuickReply({ origin: location.origin, board: 'demo', thread: '10', fields: { com: 'Owned reply' } });
    });
    assert.deepEqual(postResult, { thread: '10', post: '11' });
    assert.deepEqual(await posted(), { status: 200, result: { tid: 10, pid: 11 } });
    assert.deepEqual(counts, { upload: 1, post: 1 });
    assert.deepEqual(diagnostics, []);
  } finally {
    if (browser) await browser.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});


test('deletion capture requires HTTP 200 HTML and the unchanged success text', async () => {
  const text = '<p>The deletion was completed.</p>', diagnostics = [];
  assert.deepEqual(await ownedDeletionResponse({ status: () => 200,
    headers: () => ({ 'content-type': 'text/html; charset=utf-8' }), text: async () => text },
  value => diagnostics.push(value)), { status: 200, text });
  assert.deepEqual(diagnostics, []);
  for (const [status, mime, body, failure, type] of [
    [403, 'text/html', text, 'http', 'html'],
    [429, 'text/plain', text, 'http', 'plain'],
    [200, 'application/json', text, 'http', 'json'],
    [200, 'private-type', text, 'http', 'other'],
    [200, 'text/html', 'private response', 'content', 'html'],
    [200, 'text/html', text + 'x'.repeat(4096), 'content', 'html'],
    [200, 'text/html', new Error('private URL and capability'), 'body', 'html'],
  ]) {
    const messages = [];
    await assert.rejects(ownedDeletionResponse({ status: () => status,
      headers: () => ({ 'content-type': mime }), text: async () => {
        if (body instanceof Error) throw body;
        return body;
      } }, value => messages.push(value)), /^Error: Owned deletion response was not confirmed\.$/);
    assert.deepEqual(messages, [`OWNED_UPLOAD_RESPONSE status=${status} type=${type} stage=deletion failure=${failure}`]);
  }
});

test('invalid deletion statuses cannot enter safe diagnostics', async () => {
  const messages = [];
  for (const status of [99, 600, 200.5, '200 private-value', NaN]) {
    await assert.rejects(ownedDeletionResponse({ status: () => status }, value => messages.push(value)),
      /^Error: Invalid owned response status\.$/);
  }
  assert.deepEqual(messages, []);
});

test('deletion page capture survives deterministic native transport consumption and abort', async () => {
  const saved = globalThis.window;
  const html = (await readFile(new URL('../../apps/public/templates/delete_success.html', import.meta.url), 'utf8'))
    .replace('{{ board }}', 'demo');
  const original = new Response(html, { headers: { 'content-type': 'text/html' } });
  const calls = [], diagnostics = [];
  const fetcher = async (url, options) => { calls.push({ url, options }); return original; };
  try {
    globalThis.window = { fetch: fetcher };
    const page = { evaluate: async (fn, argument) => fn(argument) };
    const captured = await observeOwnedDeletionResponse(page, 'http://owned.invalid/demo/imgboard.php',
      value => diagnostics.push(value));
    assert.equal(await sendNativeDeletion({ board: 'demo', id: '11', origin: 'http://owned.invalid',
      fetcher: window.fetch }), true);
    assert.equal(original.bodyUsed, true);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].options.signal.aborted, true);
    assert.equal(calls[0].url, 'http://owned.invalid/demo/imgboard.php');
    assert.equal(calls[0].options.method, 'POST');
    assert.deepEqual([...calls[0].options.body], [['11', 'delete'], ['mode', 'usrdel']]);
    assert.equal(window.fetch, fetcher);
    // Read only after the real transport has consumed the original response
    // and aborted its controller. Observation must not re-fetch the mutation.
    assert.deepEqual(await captured(), { status: 200, text: html });
    assert.equal(calls.length, 1);
    assert.deepEqual(diagnostics, []);
  } finally {
    if (saved === undefined) delete globalThis.window;
    else globalThis.window = saved;
  }
});

test('browser capture retains native deletion HTML after actual transport cleanup without another POST', { timeout: 30000 }, async () => {
  const [transport, core, success] = await Promise.all([
    readFile(new URL('../../apps/public/client/native-post-deletion.js', import.meta.url)),
    readFile(new URL('../../apps/public/static/thread-watcher-core.v1.js', import.meta.url)),
    readFile(new URL('../../apps/public/templates/delete_success.html', import.meta.url), 'utf8'),
  ]);
  const html = success.replace('{{ board }}', 'demo');
  const bodies = [];
  const server = createServer((request, response) => {
    if (request.method === 'POST' && request.url === '/demo/imgboard.php') {
      let body = '';
      request.on('data', chunk => { body += chunk; });
      request.on('end', () => {
        bodies.push(body);
        response.writeHead(200, { 'content-type': 'text/html' }).end(html);
      });
    } else {
      request.resume();
      if (request.url === '/client/native-post-deletion.js') {
        response.writeHead(200, { 'content-type': 'text/javascript' }).end(transport);
      } else if (request.url === '/static/thread-watcher-core.v1.js') {
        response.writeHead(200, { 'content-type': 'text/javascript' }).end(core);
      } else response.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><title>Owned deletion response</title>');
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  let browser;
  try {
    browser = await chromium.launch({ timeout: 15000 });
    const page = await browser.newPage();
    await page.goto(origin);
    const diagnostics = [];
    const deleted = await observeOwnedDeletionResponse(page, `${origin}/demo/imgboard.php`, value => diagnostics.push(value));
    // Await the actual transport's completed cleanup before reading the capture.
    assert.equal(await page.evaluate(async () => {
      const { sendNativeDeletion } = await import('/client/native-post-deletion.js');
      return sendNativeDeletion({ origin: location.origin, board: 'demo', id: '11', fileOnly: true });
    }), true);
    assert.deepEqual(await deleted(), { status: 200, text: html });
    assert.equal(bodies.length, 1);
    assert.deepEqual([...new URLSearchParams(bodies[0])].sort(),
      [['11', 'delete'], ['mode', 'usrdel'], ['onlyimgdel', 'on']].sort());
    assert.deepEqual(diagnostics, []);
  } finally {
    if (browser) await browser.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

test('deletion page capture bounds bytes and preserves the original response', async () => {
  const saved = globalThis.window;
  const success = 'The deletion was completed.';
  try {
    for (const [bytes, failure] of [
      [new TextEncoder().encode(success.padEnd(4096)), null],
      [new TextEncoder().encode(success.padEnd(4097)), 'body'],
      [new Uint8Array([0xff]), 'body'],
      [new TextEncoder().encode('private unexpected content'), 'content'],
    ]) {
      const original = new Response(bytes, { headers: { 'content-type': 'text/html' } });
      let calls = 0;
      const fetcher = async () => { calls++; return original; };
      globalThis.window = { fetch: fetcher };
      const page = { evaluate: async (fn, argument) => fn(argument) }, diagnostics = [];
      const captured = await observeOwnedDeletionResponse(page, 'http://owned.invalid/demo/imgboard.php', value => diagnostics.push(value));
      const received = await window.fetch('http://owned.invalid/demo/imgboard.php', { method: 'POST' });
      assert.equal(received, original);
      assert.equal(window.fetch, fetcher);
      assert.deepEqual(new Uint8Array(await received.arrayBuffer()), bytes);
      if (failure) {
        await assert.rejects(captured(), /^Error: Owned deletion response was not confirmed\.$/);
        assert.deepEqual(diagnostics, [`OWNED_UPLOAD_RESPONSE status=200 type=html stage=deletion failure=${failure}`]);
      } else {
        assert.deepEqual(await captured(), { status: 200, text: success.padEnd(4096) });
        assert.deepEqual(diagnostics, []);
      }
      assert.equal(calls, 1);
    }
  } finally {
    if (saved === undefined) delete globalThis.window;
    else globalThis.window = saved;
  }
});
