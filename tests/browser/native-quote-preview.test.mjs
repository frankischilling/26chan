import test from 'node:test';
import assert from 'node:assert/strict';
import { PREVIEW_LIMITS, parseQuotePreviewSnapshot, previewContext, previewUrl } from '../../apps/public/client/native-updater-snapshot.js';
import { NativeQuotePreviewTransport, checkedQuotePreview } from '../../apps/public/client/native-quote-preview-transport.js';
import { quoteTarget, quotePreviewPosition, mobileQuoteDevice } from '../../apps/public/client/native-quote-preview.js';

const context = { origin: 'https://board.example', mediaOrigin: 'https://media.example', board: 'demo', post: '9007199254740993' };
const thread = '9007199254740992';
const content = 'Safe &lt;script&gt; &amp; text';
function snapshot(inside = content, no = context.post, parent = thread) {
  const op = no === parent;
  return { version: 1, board: 'demo', thread: parent, post: { no, file_deleted: false,
    html: `<article class="postContainer ${op ? 'opContainer' : 'replyContainer'}" id="pc${no}"><div class="post ${op ? 'op' : 'reply'}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${parent}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${inside}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><label for="delete${no}">Deletion password</label><input type="password" name="password" id="delete${no}" autocomplete="off" required><button>Delete</button></form></details></div></article>` } };
}
const parse = (value = snapshot(), request = context) => parseQuotePreviewSnapshot(JSON.stringify(value), request);
const response = (url, body = JSON.stringify(snapshot()), options = {}) => ({
  url, status: 200, redirected: false, headers: new Headers({ 'content-type': 'application/json; charset=utf-8' }),
  body: new Response(body).body, ...options,
});
function workers(record = { created: 0, terminated: 0 }, transform = value => value) {
  return () => {
    record.created++;
    const worker = { terminate() { record.terminated++; }, postMessage(job) {
      assert.equal(job.kind, 'quote-preview');
      queueMicrotask(() => worker.onmessage?.({ data: transform(parseQuotePreviewSnapshot(job.raw, job.context)) }));
    } };
    return worker;
  };
}

test('rewrite quote grammar keeps exact i64 IDs and rejects redirects, alternate origins and ambiguous paths', () => {
  const page = { origin: context.origin, board: context.board, thread };
  for (const raw of [`/demo/post/${context.post}`, `${context.origin}/demo/post/${context.post}`]) {
    assert.deepEqual(quoteTarget(raw, page), { board: 'demo', post: context.post, thread: null });
  }
  for (const raw of [`#p${context.post}`, `/demo/thread/${thread}#p${context.post}`, `${context.origin}/demo/thread/${thread}#p${context.post}`]) {
    assert.deepEqual(quoteTarget(raw, page), { board: 'demo', post: context.post, thread });
  }
  assert.deepEqual(quoteTarget('/other/post/9223372036854775807', page), { board: 'other', post: '9223372036854775807', thread: null });
  for (const raw of ['/demo/post/0', '/demo/post/01', '/demo/post/9223372036854775808', '/demo/post/+2',
    '/demo/post/1e2', '/demo/post/1.0', '/DEMO/post/1', '/demo/post/1/', '/demo/post/1?x=1', '/demo/post/1#p1',
    '/demo/thread/2#p1', '/demo/thread/01#p2', '/demo/thread/1', '1#p2', '//board.example/demo/post/1',
    'https://evil.example/demo/post/1', 'https://board.example.evil/demo/post/1', 'https://u:p@board.example/demo/post/1',
    '/demo/../demo/post/1', '/demo/post/%31', '/demo\\post\\1', '\n/demo/post/1', '/demo/post/1\n', null, 1]) {
    assert.equal(quoteTarget(raw, page), null, String(raw));
  }
  assert.equal(previewUrl(context), `https://board.example/_watch/demo/post/${context.post}`);
  assert.equal(previewContext(context).thread, null);
  for (const changed of [{ post: 1 }, { post: '01' }, { post: '1\n' }, { post: '9223372036854775808' }, { thread: '9223372036854775807' },
    { thread: 1 }, { board: '../staff' }, { origin: 'https://u:p@board.example' }, { origin: 'https://board.example/path' },
    { thread: '1\n' }, { board: 'demo\n' }, { mediaOrigin: 'data:text/plain,no' }]) assert.throws(() => previewUrl({ ...context, ...changed }));
});

test('mobile UA matching and source positioning are independent of narrow desktop layout', () => {
  for (const token of ['Mobile', 'Android', 'Dolfin', 'Opera Mobi', 'PlayStation Vita', 'Nintendo DS']) assert.equal(mobileQuoteDevice(`Test ${token} browser`), true);
  for (const token of ['Windows NT', 'Macintosh', 'iPad', 'mobile', '', undefined]) assert.equal(mobileQuoteDevice(token), false);
  const viewport = { width: 1000, height: 700, x: 20, y: 200 }, size = { width: 200, height: 100 };
  assert.deepEqual(quotePreviewPosition({ left: 100, right: 160, top: 200, bottom: 220, height: 20 }, size, viewport), { left: 185, top: 360 });
  assert.deepEqual(quotePreviewPosition({ left: 800, right: 860, top: 200, bottom: 220, height: 20 }, size, viewport), { left: 615, top: 360 });
  assert.deepEqual(quotePreviewPosition({ left: 100, right: 160, top: 200, bottom: 220, height: 20 }, size, viewport, true), { left: 120, top: 420 });
  assert.deepEqual(quotePreviewPosition({ left: 800, right: 860, top: 200, bottom: 220, height: 20 }, size, viewport, true), { left: 680, top: 420 });
  for (const top of [-1000, 10000]) {
    const positioned = quotePreviewPosition({ left: 80, right: 90, top, bottom: top + 10, height: 10 }, { width: 300, height: 400 }, { width: 100, height: 100 });
    assert.deepEqual(positioned, { left: 0, top: 0 });
  }
});

test('Preview v1 binds resolved board/thread/post and validates the entire envelope before recipes', () => {
  const result = parse();
  assert.equal(result.status, 'ok');
  assert.equal(result.snapshot.post.no, '9007199254740993');
  assert.equal(result.snapshot.thread, '9007199254740992');
  assert.equal(result.snapshot.post.html, undefined);
  assert.ok(JSON.stringify(result).includes('Safe <script> & text'));
  assert.equal(parse(snapshot(), { ...context, thread }).status, 'ok');
  assert.equal(parse(snapshot(), { ...context, thread: '1' }).status, 'invalid-preview');
  assert.equal(parse(snapshot(content, thread, thread), { ...context, post: thread }).status, 'ok');
  const max = '9223372036854775807';
  assert.equal(parse(snapshot(content, max, max), { ...context, post: max }).status, 'ok');
  for (const edit of [s => s.version = 2, s => s.extra = true, s => s.board = 'other', s => s.thread = 1,
    s => s.thread = '01', s => s.thread = '1\n', s => s.thread = '9223372036854775808', s => s.thread = '9007199254740994',
    s => s.post.no = Number(s.post.no), s => s.post.no = '9007199254740992', s => s.post.file_deleted = 0,
    s => s.post.extra = '', s => s.post.html += '<p>Second root</p>', s => s.post.html = null]) {
    const changed = snapshot(); edit(changed); assert.equal(parse(changed).status, 'invalid-preview');
  }
});

test('hostile HTML, media, credentials and oversized recipes cannot cross the parser boundary', () => {
  for (const hostile of ['<script>alert(1)</script>', '<style>body{display:none}</style>', '<iframe src="/staff"></iframe>',
    '<svg><image href="https://tracker.example/x"/></svg>', '<math><mi>x</mi></math>', '<template><img src="/tracker"></template>',
    '<img src="https://tracker.example/1.png" alt="bad">', '<img src="https://media.example/other/1s.jpg" alt="bad">',
    '<img src="https://media.example/demo/1s.jpg?track=1" alt="bad">', '<img src="https://media.example/demo/1s.jpg" alt="bad" srcset="https://tracker.example/x 2x">',
    '<a href="javascript:alert(1)">bad</a>', '<a href="//tracker.example/x">bad</a>',
    '<a href="https://u:p@host.example" rel="noopener noreferrer">bad</a>', '<span onclick="bad()">bad</span>',
    '<span style="background:url(https://tracker.example/x)">bad</span>', '<video autoplay src="https://media.example/demo/1.png"></video>']) {
    assert.equal(parse(snapshot(hostile)).status, 'invalid-preview', hostile);
  }
  assert.equal(parse(snapshot('<img src="https://media.example/demo/123s.jpg" alt="safe" width="250" height="120" loading="lazy">')).status, 'ok');
  const password = snapshot(); password.post.html = password.post.html.replace('type="password"', 'type="password" value="secret"');
  assert.equal(parse(password).status, 'invalid-preview');
  for (const inside of ['折'.repeat(Math.ceil(PREVIEW_LIMITS.bytes / 3)), '<br>'.repeat(PREVIEW_LIMITS.nodes),
    '<span>'.repeat(40) + 'deep' + '</span>'.repeat(40)]) assert.equal(parse(snapshot(inside)).status, 'invalid-preview');
  assert.equal(parseQuotePreviewSnapshot('{', context).status, 'invalid-preview');
  const forged = parse();
  forged.snapshot.post.tree.children.push({ tag: 'img', attrs: { src: 'https://tracker.example/x', alt: 'bad' }, children: [] });
  assert.equal(checkedQuotePreview(forged, context).status, 'invalid-preview');
  const rebound = parse(); rebound.snapshot.thread = '1';
  assert.equal(checkedQuotePreview(rebound, { ...context, thread }).status, 'invalid-preview');
});

test('transport fixes request authority, keeps one slot, bounds frequency and never reuses stale deleted posts', async () => {
  let clock = 0, missing = false;
  const requests = [], record = { created: 0, terminated: 0 };
  const transport = new NativeQuotePreviewTransport({ ...context, now: () => clock, createWorker: workers(record),
    fetcher: async (url, options) => {
      requests.push({ url, options });
      return response(url, undefined, missing ? { status: 404 } : {});
    } });
  const result = await transport.load({ ...context, origin: 'https://evil.example', mediaOrigin: 'https://evil.example' });
  assert.equal(result.status, 'ok'); assert.equal(result.context.thread, thread);
  assert.equal(requests[0].url, previewUrl(context));
  const { options } = requests[0];
  assert.equal(options.method, 'GET'); assert.equal(options.credentials, 'omit'); assert.equal(options.mode, 'same-origin');
  assert.equal(options.redirect, 'error'); assert.equal(options.cache, 'no-store');
  assert.deepEqual(options.headers, { Accept: 'application/json' });
  assert.deepEqual(await transport.load(context), { status: 'cooldown', retryAfter: PREVIEW_LIMITS.intervalMs });
  clock += PREVIEW_LIMITS.intervalMs; missing = true;
  assert.deepEqual(await transport.load(context), { status: 'http-error', httpStatus: 404 });
  assert.equal(record.created, 1); assert.equal(record.terminated, 1); assert.equal(requests.length, 2);
  clock += PREVIEW_LIMITS.intervalMs; missing = false;
  assert.equal((await transport.load(context)).status, 'ok'); assert.equal(record.created, 2);
  assert.equal((await transport.load({ ...context, post: '01' })).status, 'invalid-context');
});

test('streaming enforces URL, MIME, declared and actual bytes, UTF-8, and finite empty-chunk work', async () => {
  const headers = value => new Headers(value);
  for (const [name, fetcher, status] of [
    ['redirect', async url => response(url, '', { redirected: true }), 'invalid-response'],
    ['different URL', async url => response(url + '?other=1'), 'invalid-response'],
    ['MIME', async url => response(url, '', { headers: headers({ 'content-type': 'text/html' }) }), 'invalid-response'],
    ['declared length', async url => response(url, '', { headers: headers({ 'content-type': 'application/json', 'content-length': '4097' }) }), 'response-limit'],
    ['negative length', async url => response(url, '', { headers: headers({ 'content-type': 'application/json', 'content-length': '-1' }) }), 'response-limit'],
    ['actual bytes', async url => response(url, 'x'.repeat(4097)), 'response-limit'],
    ['UTF-8', async url => response(url, new Uint8Array([0xc3, 0x28])), 'invalid-encoding'],
    ['truncated UTF-8', async url => response(url, new Uint8Array([0xe6, 0x8a])), 'invalid-encoding'],
    ['unsolicited 304', async url => response(url, '', { status: 304 }), 'http-error'],
    ['missing body', async url => response(url, '', { body: null }), 'invalid-response'],
    ['empty chunks', async url => response(url, '', { body: { getReader: () => ({ read: async () => ({ value: new Uint8Array(), done: false }), cancel() {} }) } }), 'response-limit'],
  ]) {
    const record = { created: 0, terminated: 0 };
    const transport = new NativeQuotePreviewTransport({ ...context, fetcher, createWorker: workers(record), limits: { bytes: 4096 } });
    assert.equal((await transport.load(context)).status, status, name); assert.equal(record.created, 0, name);
  }
  const bytes = new TextEncoder().encode(JSON.stringify(snapshot('折😀')));
  const transport = new NativeQuotePreviewTransport({ ...context, createWorker: workers(), fetcher: async url => response(url, '', {
    body: new ReadableStream({ start(controller) {
      for (let i = 0; i < bytes.length; i++) controller.enqueue(bytes.slice(i, i + 1)); controller.close();
    } }),
  }) });
  assert.equal((await transport.load(context)).status, 'ok');
});

test('total deadlines and cancellation release slots even when fetch or readers ignore abort', async () => {
  for (const fetcher of [() => new Promise(() => {}), async url => response(url, '', {
    body: { getReader: () => ({ read: () => new Promise(() => {}), cancel() {} }) },
  })]) {
    const transport = new NativeQuotePreviewTransport({ ...context, fetcher, limits: { requestMs: 20 } });
    const pending = transport.load(context);
    assert.equal((await transport.load(context)).status, 'busy');
    assert.equal((await pending).status, 'timeout'); assert.equal(transport.active, null);
    const controller = new AbortController(), cancelled = new NativeQuotePreviewTransport({ ...context, fetcher });
    const work = cancelled.load(context, { signal: controller.signal }); controller.abort();
    assert.equal((await work).status, 'cancelled'); assert.equal(cancelled.active, null);
  }
  let deliver, discarded = 0;
  const record = { created: 0, terminated: 0 };
  const transport = new NativeQuotePreviewTransport({ ...context, createWorker: workers(record), fetcher: () => new Promise(resolve => { deliver = resolve; }) });
  const pending = transport.load(context); transport.cancel();
  assert.equal((await pending).status, 'cancelled');
  deliver(response(previewUrl(context), '', { body: { cancel() { discarded++; } } }));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(discarded, 1); assert.equal(record.created, 0);
});

test('hung and hostile workers terminate without exposing unvalidated recipes', async () => {
  for (const mode of ['hung', 'hostile', 'cancel', 'error']) {
    let terminated = 0, transport;
    const createWorker = () => ({ terminate() { terminated++; }, postMessage() {
      if (mode === 'hostile') {
        const result = parse(); result.snapshot.post.tree.children.push({ tag: 'script', attrs: {}, children: ['bad()'] });
        this.onmessage({ data: result });
      }
      if (mode === 'cancel') transport.cancel();
      if (mode === 'error') this.onerror({ preventDefault() {} });
    } });
    transport = new NativeQuotePreviewTransport({ ...context, createWorker, fetcher: async url => response(url), limits: { parseMs: 10 } });
    assert.equal((await transport.load(context)).status, { hung: 'parse-timeout', hostile: 'invalid-preview', cancel: 'cancelled', error: 'worker-error' }[mode]);
    assert.equal(terminated, 1); assert.equal(transport.active, null);
  }
});
