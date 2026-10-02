import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownedUploadResponse } from './owned-upload-response.mjs';

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
});
