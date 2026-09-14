import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { useUpdaterTail } from '../../apps/public/client/native-updater-tail.js';
import { parseUpdaterSnapshot, updaterUrl, UPDATER_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { NativeUpdaterTransport } from '../../apps/public/client/native-updater-transport.js';

const context = { origin: 'https://board.example', board: 'demo', thread: '9007199254740992', mediaOrigin: 'https://media.example' };
const html = (no, inside = 'Safe &lt;script&gt; &amp; text') => `<article class="postContainer ${no === context.thread ? 'opContainer' : 'replyContainer'}" id="pc${no}"><div class="post ${no === context.thread ? 'op' : 'reply'}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${context.thread}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${inside}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><label for="delete${no}">Deletion password</label><input id="delete${no}" name="password" type="password" minlength="8" maxlength="128" autocomplete="off" required><button>Delete post</button></form></details></div></article>`;
function snapshot(inside) {
  return { version: 2, tail_size: 0, tail_id: null, board: 'demo', thread: context.thread, closed: false, archived: false, sticky: false,
    replies: 1, images: 0, posts: [context.thread, '9007199254740993'].map(no => ({ no, file_deleted: false, html: html(no, inside) })) };
}
function parse(s, c = context) { return parseUpdaterSnapshot(JSON.stringify(s), c); }

test('owned rendering produces only data with exact adjacent large IDs and decoded text', () => {
  const result = parse(snapshot()); assert.equal(result.status, 'ok');
  assert.deepEqual(result.snapshot.posts.map(p => p.no), ['9007199254740992', '9007199254740993']);
  assert.ok(JSON.stringify(result).includes('Safe <script> & text'));
  assert.equal(updaterUrl(context), 'https://board.example/_watch/demo/thread/9007199254740992/posts');
  for (const bad of [{ thread: null }, { thread: '01' }, { thread: '9223372036854775808' }, { board: '../staff' },
    { origin: 'https://u:p@board.example' }, { origin: 'https://board.example/path' }, { mediaOrigin: 'javascript:alert(1)' }]) {
    assert.throws(() => updaterUrl({ ...context, ...bad }));
  }
});

test('active content, foreign namespaces, unapproved fetches and credential-bearing forms are rejected', () => {
  for (const content of ['<script>alert(1)</script>', '<svg><a href="/">bad</a></svg>', '<style>body{display:none}</style>',
    '<img src="https://tracker.example/a.png" alt="bad">', '<iframe src="/staff"></iframe>', '<a href="javascript:alert(1)">bad</a>',
    '<a href="//tracker.example/">bad</a>', '<a href="https://u:p@host.example/" rel="noopener noreferrer">bad</a>',
    '<span onclick="alert(1)">bad</span>', '<span style="color:red">bad</span>', '<div id="pi123">collision</div>',
    '<template><img src="/tracker"></template>']) assert.equal(parse(snapshot(content)).status, 'invalid-snapshot', content);
  for (const [from, to] of [['/demo/delete', '/staff/remove'], ['value="9007199254740993"', 'value="1"'],
    ['type="password"', 'type="password" value="leaked"'], ['<form method="post"', '<form target="_blank" method="post"'],
    ['class="postMessage"', 'class="arbitraryClass"']]) {
    const s = snapshot(); s.posts[1].html = s.posts[1].html.replace(from, to);
    assert.equal(parse(s).status, 'invalid-snapshot', to);
  }
});

test('approved formatting and normalized media are allowed without accepting unrelated URLs', () => {
  const content = '<span class="quote">&gt;green</span><br><span class="spoiler" tabindex="0" aria-label="Spoiler; focus to reveal">secret</span>'
    + '<a class="quotelink" href="/other/post/42">&gt;&gt;&gt;/other/42</a>'
    + '<a href="https://example.org/path?q=test" rel="nofollow noreferrer noopener">link</a>'
    + '<a class="fileThumb" href="https://media.example/demo/123.png" target="_blank" rel="noopener noreferrer"><img src="https://media.example/demo/123s.jpg" alt="file" width="250" height="100" loading="lazy"></a>';
  assert.equal(parse(snapshot(content)).status, 'ok');
  assert.equal(parse(snapshot(content), { ...context, mediaOrigin: '' }).status, 'invalid-snapshot');
  assert.equal(parse(snapshot(content.replace('/123s.jpg', '/../private.png'))).status, 'invalid-snapshot');
  const longUrl = `https://example.org/${encodeURIComponent('折'.repeat(4000))}`;
  assert.equal(parse(snapshot(`<a href="${longUrl}" rel="nofollow noreferrer noopener">${longUrl}</a>`)).status, 'ok');
});

test('source multiline markup crosses only the finite inert snapshot grammar', () => {
  for (const content of [
    '<s>first<br><s>&lt;script&gt;second</s></s>',
    '<pre class="prettyprint">first<br>second</pre>',
    '<span class="sjis">a  b<br>c</span>',
    '<s>first<pre class="prettyprint">crossed</s>tail</pre>',
    '<s><a class="quotelink" href="/demo/post/42">&gt;&gt;42</a></s>',
  ]) {
    const result = parse(snapshot(content));
    assert.equal(result.status, 'ok', content);
    assert.ok(JSON.stringify(result.snapshot.posts[0].tree).length > 0);
  }
  for (const content of [
    '<s onclick="bad()">bad</s>', '<s style="display:none">bad</s>',
    '<pre>bad</pre>', '<pre class="quote">bad</pre>',
    '<pre class="prettyprint" src="https://tracker.example/">bad</pre>',
    '<span class="sjis" onmouseover="bad()">bad</span>',
    '<pre class="prettyprint"><script>bad()</script></pre>',
  ]) assert.equal(parse(snapshot(content)).status, 'invalid-snapshot', content);
});

test('partial, oversized, duplicate, unordered and mismatched snapshots fail as a whole', () => {
  for (const edit of [s => s.posts.pop(), s => s.posts.reverse(), s => s.posts[1].no = s.posts[0].no,
    s => s.board = 'other', s => s.closed = 1, s => s.version = 99, s => s.images = 2,
    s => s.extra = 1, s => s.posts[1].html = s.posts[1].html.replace('id="pi9007199254740993"', 'id="p9007199254740993"'),
    s => s.posts[1].html += '<p>extra root</p>', s => s.posts = Array(1002).fill(s.posts[0])]) {
    const s = snapshot(); edit(s); assert.equal(parse(s).status, 'invalid-snapshot');
  }
  assert.equal(parseUpdaterSnapshot('{', context).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('x'.repeat(UPDATER_LIMITS.bytes))).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('<span>'.repeat(40) + 'deep' + '</span>'.repeat(40))).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('<br>'.repeat(100001))).status, 'invalid-snapshot');
});

test('bounded generated hostile text remains text after HTML entity decoding', () => {
  let seed = 71823;
  const tokens = ['<script>', '</script>', '<img src=x onerror=alert(1)>', '&lt;', '&#x3c;', '"', "'", '&', '😀', '折', ' ', '\n'];
  function comment(tree) {
    if (typeof tree === 'string') return null;
    if (tree.attrs.class === 'postMessage') return tree;
    return tree.children.map(comment).find(Boolean);
  }
  for (let run = 0; run < 256; run++) {
    let text = '';
    for (let part = 0; part < 32; part++) {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      text += tokens[seed % tokens.length];
    }
    const escaped = text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
    const result = parse(snapshot(escaped)); assert.equal(result.status, 'ok');
    const node = comment(result.snapshot.posts[1].tree);
    assert.ok(node.children.every(child => typeof child === 'string'));
    assert.equal(node.children.join(''), text);
  }
});

function workerFactory(record) {
  return () => {
    const worker = { terminate() { record.terminated++; }, postMessage(job) {
      queueMicrotask(() => worker.onmessage?.({ data: parseUpdaterSnapshot(job.raw, job.context) }));
    } }; record.created++; return worker;
  };
}
async function serverFor(t, handler) {
  const server = createServer(handler); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  return `http://127.0.0.1:${server.address().port}`;
}
test('actual HTTP streams omit credentials, enforce redirects/MIME/byte bounds and terminate parsers', async t => {
  let mode = 'ok', redirected = 0;
  const origin = await serverFor(t, (req, res) => {
    assert.equal(req.headers.cookie, undefined); assert.equal(req.headers.authorization, undefined);
    if (req.url === '/redirect-target') { redirected++; res.end('healthy'); return; }
    if (mode === 'redirect') { res.writeHead(302, { location: '/redirect-target' }); res.end(); return; }
    if (mode === 'missing') { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': mode === 'mime' ? 'text/html' : 'application/json' });
    res.end(mode === 'bad' ? '{' : mode === 'encoding' ? Buffer.from([0xff]) : mode === 'large' ? 'x'.repeat(8192) : JSON.stringify(snapshot()));
  });
  // Healthy allowed-context control for the redirect destination.
  assert.equal(await (await fetch(`${origin}/redirect-target`)).text(), 'healthy');
  const record = { created: 0, terminated: 0 };
  let now = 0;
  const transport = new NativeUpdaterTransport({ ...context, origin, now: () => now, limits: { bytes: 4096 }, createWorker: workerFactory(record),
    fetcher: (url, options) => { assert.equal(options.credentials, 'omit'); assert.equal(options.redirect, 'error'); assert.equal(options.mode, 'same-origin'); return fetch(url, options); } });
  for (const [value, expected] of [['ok', 'ok'], ['redirect', 'network-error'], ['missing', 'http-error'], ['mime', 'invalid-response'],
    ['bad', 'invalid-snapshot'], ['encoding', 'invalid-encoding'], ['large', 'response-limit']]) {
    mode = value; now += 1000;
    assert.equal((await transport.refresh()).status, expected, value);
  }
  assert.equal(redirected, 1); assert.equal(record.created, 2); assert.equal(record.terminated, 2);
});

test('timeout and cancellation settle even when fetch or the reader ignores abort', async () => {
  for (const fetcher of [() => new Promise(() => {}), async url => ({ url, status: 200, headers: new Headers({ 'content-type': 'application/json' }),
    body: new ReadableStream({ start() {} }) })]) {
    const transport = new NativeUpdaterTransport({ ...context, fetcher, limits: { requestMs: 20 } });
    const pending = transport.refresh(); assert.equal((await transport.refresh()).status, 'busy');
    assert.equal((await pending).status, 'timeout'); assert.equal(transport.active, null);
    assert.equal((await transport.refresh()).status, 'cooldown');
    const cancelled = new NativeUpdaterTransport({ ...context, fetcher });
    const work = cancelled.refresh(); cancelled.cancel(); assert.equal((await work).status, 'cancelled');
  }
});

test('hung or hostile worker results are terminated and cannot become DOM instructions', async () => {
  for (const mode of ['hung', 'hostile', 'cancel']) {
    let terminated = 0, transport;
    const createWorker = () => ({ terminate() { terminated++; }, postMessage() {
      if (mode === 'hostile') this.onmessage({ data: { status: 'ok', snapshot: { ...snapshot(), posts: [{ no: context.thread, tree: { tag: 'script', attrs: {}, children: [] } }] } } });
      if (mode === 'cancel') transport.cancel();
    } });
    transport = new NativeUpdaterTransport({ ...context, createWorker, limits: { parseMs: 10 },
      fetcher: async url => ({ url, status: 200, headers: new Headers({ 'content-type': 'application/json' }), body: new Response(JSON.stringify(snapshot())).body }) });
    assert.equal((await transport.refresh()).status, { hung: 'parse-timeout', hostile: 'invalid-snapshot', cancel: 'cancelled' }[mode]);
    assert.equal(terminated, 1); assert.equal(transport.active, null);
  }
});

function rangedSnapshot(count, size, tail = false) {
  const ids = Array.from({ length: count + 1 }, (_, i) => String(BigInt(context.thread) + BigInt(i)));
  const s = snapshot(); s.replies = count; s.tail_size = size;
  s.tail_id = tail ? ids[count - size] : null;
  s.posts = (tail ? [ids[0], ...ids.slice(-size)] : ids).map(no => ({ no, file_deleted: false, html: html(no) }));
  return s;
}
const tokenFor = value => `"${createHash('sha256').update(JSON.stringify(value)).digest('hex')}"`;
const modified = 'Mon, 14 Sep 2026 00:00:00 GMT';
function sendSnapshot(req, res, value) {
  const etag = tokenFor(value);
  const headers = { 'content-type': 'application/json', etag, 'last-modified': modified };
  if (req.headers['if-none-match'] === etag) { res.writeHead(304, headers); res.end(); }
  else { res.writeHead(200, headers); res.end(JSON.stringify(value)); }
}

test('tail metadata retains full counts and exact omitted boundaries while rejecting partial or inconsistent representations', () => {
  const s = rangedSnapshot(4, 2, true);
  assert.equal(parse(s).status, 'ok'); assert.equal(s.tail_id, '9007199254740994');
  for (const edit of [s => s.tail_id = s.posts[1].no, s => s.tail_id = context.thread,
    s => s.tail_size = 1, s => s.replies = 3, s => s.tail_id = 0, s => s.tail_size = 1.5,
    s => s.tail_size = 1001, s => s.replies = 1001, s => s.tail_id = '9223372036854775808',
    s => s.tail_id = null, s => delete s.tail_size]) {
    const candidate = structuredClone(s); edit(candidate); assert.equal(parse(candidate).status, 'invalid-snapshot');
  }
  assert.equal(updaterUrl(context, true), 'https://board.example/_watch/demo/thread/9007199254740992/posts-tail');
});

test('tail selection follows reply-window age and uses full responses for disabled, stale or invalid timing', () => {
  assert.equal(useUpdaterTail(2, [1000, 2000, 3000], 10000, 17000), true);
  assert.equal(useUpdaterTail(2, [1000, 2000, 3000], 10000, 18000), false);
  assert.equal(useUpdaterTail(2, [1000], 10000, 100000), true);
  for (const values of [[0, [1000], 10000, 11000], [2, [NaN, 2000], 10000, 11000],
    [2, [1000], 10000, 9999], [1001, [], 10000, 11000]]) assert.equal(useUpdaterTail(...values), false);
  // Avoid signed 32-bit timestamp wrapping after 2038.
  assert.equal(useUpdaterTail(1, [3000000000000], 3000000010000, 3000000011000), true);
});

test('actual full and tail HTTP validators remain separate and 304 avoids parsing or replaying a snapshot', async t => {
  let count = 4, clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    assert.equal(req.headers.cookie, undefined); assert.equal(req.headers.authorization, undefined);
    requests.push({ path: req.url, tag: req.headers['if-none-match'], date: req.headers['if-modified-since'] });
    sendSnapshot(req, res, rangedSnapshot(count, 2, req.url.endsWith('posts-tail')));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock });
  const known = new Set(rangedSnapshot(4, 2).posts.map(p => p.no));
  assert.equal((await transport.refresh()).status, 'ok'); clock += 1001;
  assert.equal((await transport.refresh()).status, 'not-modified'); clock += 1001;
  assert.equal((await transport.refresh({ tail: true, known })).status, 'ok'); clock += 1001;
  assert.equal((await transport.refresh({ tail: true, known })).status, 'not-modified'); clock += 1001;
  assert.equal(record.created, 2); assert.equal(record.terminated, 2);
  assert.equal(requests[0].date, '0'); assert.equal(requests[0].tag, undefined);
  assert.equal(requests[1].tag, tokenFor(rangedSnapshot(4, 2)));
  assert.equal(requests[2].date, '0'); assert.equal(requests[2].tag, undefined);
  assert.equal(requests[3].tag, tokenFor(rangedSnapshot(4, 2, true))); assert.equal(requests[3].date, modified);
  count = 5;
  assert.equal((await transport.refresh({ tail: true, known })).snapshot.replies, 5);
  transport.invalidate(); clock += 1001;
  assert.equal((await transport.refresh()).status, 'ok'); assert.equal(requests.at(-1).tag, undefined);
});

test('an actual missing tail boundary retries one full response inside the same refresh slot', async t => {
  const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => { requests.push(req.url); sendSnapshot(req, res, rangedSnapshot(6, 2, req.url.endsWith('posts-tail'))); });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => 0 });
  const known = new Set(rangedSnapshot(2, 0).posts.map(p => p.no));
  const result = await transport.refresh({ tail: true, known });
  assert.equal(result.status, 'ok'); assert.equal(result.snapshot.tail_id, null); assert.equal(result.snapshot.posts.length, 7);
  assert.deepEqual(requests.map(url => url.split('/').at(-1)), ['posts-tail', 'posts']);
  assert.equal(record.created, 2); assert.equal(record.terminated, 2);
  assert.equal((await transport.refresh()).status, 'cooldown');
});

test('tail fallback shares an aggregate byte ceiling with a healthy complete-response control', async t => {
  const full = rangedSnapshot(6, 2), tail = rangedSnapshot(6, 2, true), record = { created: 0, terminated: 0 };
  const bytes = Buffer.byteLength(JSON.stringify(full)) + Buffer.byteLength(JSON.stringify(tail)) - 1;
  const origin = await serverFor(t, (req, res) => sendSnapshot(req, res, req.url.endsWith('posts-tail') ? tail : full));
  const make = () => new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), limits: { bytes } });
  assert.equal((await make().refresh()).status, 'ok');
  assert.equal((await make().refresh({ tail: true, known: new Set([context.thread]) })).status, 'response-limit');
});

test('tail 404 retries full, full 404 is terminal, and other errors or unsolicited 304 do not become success', async t => {
  let mode = 'missing-tail', clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    requests.push(req.url);
    if (mode === 'unsolicited') { res.writeHead(304); res.end(); }
    else if (mode === 'missing-all' || (mode === 'missing-tail' && req.url.endsWith('posts-tail'))) { res.writeHead(404); res.end(); }
    else if (mode === 'failure') { res.writeHead(503); res.end(); }
    else sendSnapshot(req, res, rangedSnapshot(4, 2));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock });
  const options = { tail: true, known: new Set([context.thread]) };
  assert.equal((await transport.refresh(options)).status, 'ok'); assert.equal(requests.length, 2); clock += 1001;
  mode = 'missing-all'; assert.deepEqual(await transport.refresh(options), { status: 'http-error', httpStatus: 404 }); assert.equal(requests.length, 4); clock += 1001;
  mode = 'failure'; assert.deepEqual(await transport.refresh(options), { status: 'http-error', httpStatus: 503 }); assert.equal(requests.length, 5); clock += 1001;
  transport.invalidate(); mode = 'unsolicited'; assert.equal((await transport.refresh()).status, 'invalid-response');
});

test('a hung full fallback settles at the original deadline and a fresh healthy request still works', async t => {
  let hang = false, clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    requests.push(req.url);
    if (hang && !req.url.endsWith('posts-tail')) return;
    sendSnapshot(req, res, rangedSnapshot(6, 2, req.url.endsWith('posts-tail')));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock, limits: { requestMs: 150 } });
  assert.equal((await transport.refresh()).status, 'ok'); clock += 1001; hang = true;
  assert.equal((await transport.refresh({ tail: true, known: new Set([context.thread]) })).status, 'timeout');
  assert.equal(requests.length, 3); clock += 1001; hang = false;
  assert.equal((await transport.refresh()).status, 'not-modified');
  assert.equal(requests.length, 4);
});
