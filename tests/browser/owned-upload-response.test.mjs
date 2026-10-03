import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { chromium } from '@playwright/test';
import { observeOwnedUploadResponse, ownedUploadResponse } from './owned-upload-response.mjs';

const response = (status, type, json) => ({ status: () => status, headers: () => ({ 'content-type': type }), json });

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
