import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { parseUpdaterSnapshot, updaterUrl, UPDATER_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { NativeUpdaterTransport } from '../../apps/public/client/native-updater-transport.js';

const context = { origin: 'https://board.example', board: 'demo', thread: '9007199254740992', mediaOrigin: 'https://media.example' };
const html = (no, inside = 'Safe &lt;script&gt; &amp; text') => `<article class="postContainer ${no === context.thread ? 'opContainer' : 'replyContainer'}" id="pc${no}"><div class="post ${no === context.thread ? 'op' : 'reply'}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${context.thread}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${inside}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><label for="delete${no}">Deletion password</label><input id="delete${no}" name="password" type="password" minlength="8" maxlength="128" autocomplete="off" required><button>Delete post</button></form></details></div></article>`;
function snapshot(inside) {
  return { version: 1, board: 'demo', thread: context.thread, closed: false, archived: false, sticky: false,
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

test('partial, oversized, duplicate, unordered and mismatched snapshots fail as a whole', () => {
  for (const edit of [s => s.posts.pop(), s => s.posts.reverse(), s => s.posts[1].no = s.posts[0].no,
    s => s.board = 'other', s => s.closed = 1, s => s.version = 2, s => s.images = 2,
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
