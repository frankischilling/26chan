import test from 'node:test';
import assert from 'node:assert/strict';
import {
  QUICK_REPLY_UPLOAD_LIMITS,
  cancelQuickReplyUpload,
  checkQuickReplyUpload,
  commentLengthWarning,
  parseQuickReplyUpload,
  quoteInsertion,
  postingResult,
  sendQuickReply,
  uploadQuickReplyFile,
} from '../../apps/public/client/native-quick-reply-transport.js';

const upload = {
  upload_id: '1'.repeat(32),
  upload_capability: '2'.repeat(64),
  resto: '10',
  state: 'queued',
};

function uploadResponse(value, { status = 200, contentType = 'application/json' } = {}) {
  return new Response(JSON.stringify(value), { status, headers: { 'content-type': contentType } });
}

test('the source comment advisory counts UTF-8 bytes against the configured character limit', () => {
  assert.equal(commentLengthWarning('abcd', '4'), '');
  assert.equal(commentLengthWarning('😀', '4'), '');
  assert.equal(commentLengthWarning('😀a', '4'), 'Error: Comment too long (5/4).');
  assert.equal(commentLengthWarning('a\r\nb', '3'), 'Error: Comment too long (4/3).');
  assert.equal(commentLengthWarning('é'.repeat(8001), '16000'), 'Error: Comment too long (16002/16000).');
  for (const limit of [undefined, null, '', '0', '01', '16001', '1e3', 'Infinity', '1; color:red']) assert.equal(commentLengthWarning('xx', limit), '');
});

test('source quotes replace selection and preserve exact large IDs', () => {
  assert.deepEqual(quoteInsertion('beforeafter', 6, 6, '9223372036854775807', ' a\r\n\nb '), {
    value: 'before>>9223372036854775807\n>a\n>b\nafter', caret: 34,
  });
  assert.equal(quoteInsertion('xxzz', 0, 2, '42').value, '>>42\nzz');
});
test('posting responses never round IDs or admit alternate targets and fields', () => {
  assert.deepEqual(postingResult('{"tid":9007199254740992,"pid":9223372036854775807}', '9007199254740992'),
    { thread: '9007199254740992', post: '9223372036854775807' });
  assert.deepEqual(postingResult('{"error":"<script>not markup</script>"}', '1'), { error: '<script>not markup</script>' });
  for (const text of ['{"tid":0,"pid":2}', '{"tid":1,"pid":1}', '{"tid":1,"pid":2,"extra":1}',
    '{"tid":1,"pid":9223372036854775808}', '{"tid":1,"pid":"2"}', '{"tid":1,"pid":2e3}', '{"error":[]}', '{"error":""}',
    '{"error":"one","error":"two"}', 'x'.repeat(8193)]) {
    assert.throws(() => postingResult(text, '1'));
  }
});
test('posting targets one fixed path with source multipart fields and no redirects', async () => {
  let count = 0;
  const result = await sendQuickReply({ origin: 'https://board.example', board: 'demo', thread: '10', fields: { com: 'text', pwd: 'owned-password' },
    fetcher: async (url, options) => {
      count++; assert.equal(url, 'https://board.example/demo/imgboard.php'); assert.equal(options.method, 'POST');
      assert.equal(options.redirect, 'error'); assert.equal(options.credentials, 'same-origin'); assert.equal(options.headers.Accept, 'application/json');
      assert.equal(options.body.get('pwd'), 'owned-password'); assert.equal(options.body.get('resto'), '10'); assert.equal(options.body.get('mode'), 'regist');
      return new Response('{"tid":10,"pid":11}', { headers: { 'content-type': 'application/json' } });
    } });
  assert.deepEqual(result, { thread: '10', post: '11' }); assert.equal(count, 1);
});
test('malformed, oversized and interrupted replies do not cause retry', async () => {
  for (const text of ['not json', 'x'.repeat(8193)]) {
    let count = 0;
    await assert.rejects(sendQuickReply({ origin: 'https://board.example', board: 'demo', thread: '10', fields: {},
      fetcher: async () => { count++; return new Response(text, { headers: { 'content-type': 'application/json' } }); } }), /Check the thread/);
    assert.equal(count, 1);
  }
  const controller = new AbortController(); let count = 0;
  const pending = sendQuickReply({ origin: 'https://board.example', board: 'demo', thread: '10', fields: {}, signal: controller.signal,
    fetcher: () => { count++; return new Promise(() => {}); } });
  controller.abort(); await assert.rejects(pending, /Check the thread/); assert.equal(count, 1);
});

test('invalid fields fail before fetch and canceled streams release their readers', async () => {
  let calls = 0, canceled = false;
  const common = { origin: 'https://board.example', board: 'demo', thread: '10', fields: {}, fetcher: async () => { calls++; throw new Error('unexpected'); } };
  for (const fields of [{ com: '😀'.repeat(22501) }, { pwd: new Blob(['x']) }]) {
    await assert.rejects(sendQuickReply({ ...common, fields }));
  }
  await assert.rejects(sendQuickReply({ ...common, board: '../other' })); assert.equal(calls, 0);
  await assert.rejects(sendQuickReply({ ...common, fetcher: async () => new Response(new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array(8193)); }, cancel() { canceled = true; },
  }), { headers: { 'content-type': 'application/json' } }) }), /Check the thread/);
  assert.equal(canceled, true);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(sendQuickReply({ ...common, signal: controller.signal }), /Check the thread/); assert.equal(calls, 0);
});

test('inline upload receipts are exact, canonical and bound to the selected thread', () => {
  assert.deepEqual(parseQuickReplyUpload(JSON.stringify(upload), '10'), upload);
  assert.deepEqual(parseQuickReplyUpload(JSON.stringify({ ...upload, state: 'approved' }), '10'), { ...upload, state: 'approved' });
  for (const value of [
    { ...upload, resto: '11' },
    { ...upload, resto: 10 },
    { ...upload, upload_id: 'A'.repeat(32) },
    { ...upload, upload_capability: '2'.repeat(63) },
    { ...upload, state: 'published' },
    { ...upload, extra: true },
    { error: '<b>not a receipt</b>' },
  ]) assert.throws(() => parseQuickReplyUpload(JSON.stringify(value), '10'), /Invalid upload response/);
  assert.throws(() => parseQuickReplyUpload('x'.repeat(QUICK_REPLY_UPLOAD_LIMITS.responseBytes + 1), '10'), /Invalid upload response/);
});

test('inline selection streams one bounded file to the fixed board upload endpoint without retries', async () => {
  const calls = [];
  const file = new File([new Uint8Array([1, 2, 3])], 'fold.png', { type: 'image/png' });
  const result = await uploadQuickReplyFile({
    origin: 'https://board.example', board: 'demo', thread: '10', file,
    fetcher: async (url, options) => {
      calls.push({ url, options });
      assert.equal(url, 'https://board.example/demo/upload');
      assert.equal(options.method, 'POST');
      assert.equal(options.redirect, 'error');
      assert.equal(options.credentials, 'same-origin');
      assert.equal(options.headers.Accept, 'application/json');
      assert.deepEqual([...options.body.keys()], ['resto', 'upfile']);
      assert.equal(options.body.get('resto'), '10');
      const bodyFile = options.body.get('upfile');
      assert.equal(bodyFile.name, file.name);
      assert.equal(bodyFile.type, file.type);
      assert.equal(bodyFile.size, file.size);
      assert.deepEqual(new Uint8Array(await bodyFile.arrayBuffer()), new Uint8Array([1, 2, 3]));
      return uploadResponse(upload);
    },
  });
  assert.deepEqual(result, upload); assert.equal(calls.length, 1);

  let oversizedCalls = 0;
  const oversized = new File([new Uint8Array(QUICK_REPLY_UPLOAD_LIMITS.bytes + 1)], 'large.png', { type: 'image/png' });
  await assert.rejects(uploadQuickReplyFile({
    origin: 'https://board.example', board: 'demo', thread: '10', file: oversized,
    fetcher: async () => { oversizedCalls++; return uploadResponse(upload); },
  }), /8 MiB/);
  assert.equal(oversizedCalls, 0);
});

test('status and cancel retain the exact capability, use fixed current-board paths and never retry malformed responses', async () => {
  let statusCalls = 0;
  const approved = await checkQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
    fetcher: async (url, options) => {
      statusCalls++;
      assert.equal(url, 'https://board.example/demo/upload/status');
      assert.equal(options.method, 'POST'); assert.equal(options.redirect, 'error'); assert.equal(options.credentials, 'same-origin');
      assert.equal(options.headers.Accept, 'application/json');
      assert.equal(new URLSearchParams(options.body).get('upload_id'), upload.upload_id);
      assert.equal(new URLSearchParams(options.body).get('upload_capability'), upload.upload_capability);
      assert.equal(new URLSearchParams(options.body).get('resto'), '10');
      return uploadResponse({ ...upload, state: 'approved' });
    },
  });
  assert.equal(approved.state, 'approved'); assert.equal(statusCalls, 1);

  let cancelCalls = 0;
  assert.deepEqual(await cancelQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: approved,
    fetcher: async (url, options) => {
      cancelCalls++;
      assert.equal(url, 'https://board.example/demo/upload/cancel');
      assert.equal(options.method, 'POST'); assert.equal(options.headers.Accept, 'application/json');
      return uploadResponse({ cancelled: true });
    },
  }), { cancelled: true });
  assert.equal(cancelCalls, 1);

  let malformedCalls = 0;
  await assert.rejects(checkQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
    fetcher: async () => { malformedCalls++; return new Response('<html>no</html>', { status: 503, headers: { 'content-type': 'text/html' } }); },
  }), /Upload status unavailable/);
  assert.equal(malformedCalls, 1);

  for (const changed of [
    { ...upload, upload_id: '3'.repeat(32), state: 'approved' },
    { ...upload, upload_capability: '4'.repeat(64), state: 'approved' },
  ]) {
    await assert.rejects(checkQuickReplyUpload({
      origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
      fetcher: async () => uploadResponse(changed),
    }), /Upload status unavailable/);
  }
});

test('only exact server JSON errors reach callers; transport exception text stays private', async () => {
  await assert.rejects(checkQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
    fetcher: async () => { throw new Error('sensitive transport detail'); },
  }), error => error.message === 'Upload status unavailable. Try again.');
  await assert.rejects(cancelQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
    fetcher: async () => uploadResponse({ error: 'Upload is already in use.' }, { status: 409 }),
  }), error => error.message === 'Upload is already in use.');
});

test('upload deadlines and caller cancellation settle even when fetch or response readers ignore abort', async () => {
  const file = new File([new Uint8Array([1])], 'held.png', { type: 'image/png' });
  const controller = new AbortController();
  const heldFetch = uploadQuickReplyFile({
    origin: 'https://board.example', board: 'demo', thread: '10', file, signal: controller.signal,
    fetcher: () => new Promise(() => {}),
  });
  controller.abort();
  await assert.rejects(heldFetch, /Upload failed/);

  const statusController = new AbortController();
  const heldReader = {
    read: () => new Promise(() => {}),
    cancel: () => new Promise(() => {}),
  };
  const heldStatus = checkQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload, signal: statusController.signal,
    fetcher: async () => ({
      ok: true, status: 200,
      headers: new Headers({ 'content-type': 'application/json' }),
      body: { getReader: () => heldReader, cancel: () => Promise.resolve() },
    }),
  });
  statusController.abort();
  await assert.rejects(heldStatus, /Upload status unavailable/);

  assert.equal((await checkQuickReplyUpload({
    origin: 'https://board.example', board: 'demo', thread: '10', receipt: upload,
    fetcher: async () => uploadResponse({ ...upload, state: 'approved' }),
  })).state, 'approved');
});
