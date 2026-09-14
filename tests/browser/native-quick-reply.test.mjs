import test from 'node:test';
import assert from 'node:assert/strict';
import { quoteInsertion, postingResult, sendQuickReply } from '../../apps/public/client/native-quick-reply-transport.js';

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
