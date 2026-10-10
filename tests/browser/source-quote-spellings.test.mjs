import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import { quoteTarget, postLinkUrl, parseQuotePreviewSnapshot, parseUpdaterSnapshot, previewUrl } from '../../apps/public/client/native-updater-snapshot.js';
import { NativeQuotePreviewTransport, checkedQuotePreview } from '../../apps/public/client/native-quote-preview-transport.js';
import { quotePostId } from '../../apps/public/client/native-quote-identity.js';

const fixture = JSON.parse(await readFile(new URL('../../fixtures/source-quote-spellings.json', import.meta.url), 'utf8'));
const page = { origin: 'https://board.example', board: 'g', thread: '100' };
function source(names, globals, expression) {
  const code = names.map(name => {
    const { text, sha256 } = fixture.excerpts[name];
    assert.equal(createHash('sha256').update(text).digest('hex'), sha256);
    return text;
  }).join('\n');
  const context = vm.createContext(globals, { codeGeneration: { strings: false, wasm: false } });
  return vm.runInContext(`${code}\n${expression}`, context, { timeout: 50 });
}
const html = no => `<article class="postContainer replyContainer" id="pc${no}"><div class="post reply" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span></div><blockquote class="postMessage" id="m${no}">Remote safe text</blockquote></div></article>`;
const packet = (no = '123', thread = '100') => ({ version: 1, board: 'g', thread,
  post: { no, file_deleted: false, html: html(no) } });

test('original hover uses lexical DOM IDs and sends missing aliases to remote lookup', () => {
  for (const digits of ['123', '000123']) {
    const looked = [], remote = [], marks = [];
    const post = { id: 'p123', parentNode: {}, getBoundingClientRect: () => ({ top: 10, bottom: 30 }) };
    source(['QuotePreview.init', 'QuotePreview.resolve'], {
      QuotePreview: { showRemote: (...args) => remote.push(args.slice(1)), show() { throw Error('unexpected popup'); } },
      Main: { board: 'g' }, UA: { hasCORS: true }, location: { hash: '' },
      document: { documentElement: { clientHeight: 700 }, getElementById(id) { looked.push(id); return id === 'p123' ? post : null; } },
      $: { hasClass: () => false, addClass: (node, cls) => marks.push(cls) },
      link: { getAttribute: name => name === 'href' ? `/g/thread/100#p${digits}` : null },
    }, 'QuotePreview.init(); QuotePreview.resolve(link);');
    assert.deepEqual(looked, [`p${digits}`]);
    assert.deepEqual(marks, digits === '123' ? ['highlight'] : []);
    assert.deepEqual(remote, digits === '123' ? [] : [['g', '100', '000123']]);
  }
});

test('original backlinks preserve lexical target misses and OP comparisons on canonical page IDs', () => {
  for (const digits of ['100', '000100', '000123']) {
    const looked = [], link = { textContent: `>>${digits}`, getAttribute: () => `#p${digits}` };
    source(['Parser.parseBacklinks'], { Parser: {}, Main: { tid: '100' },
      document: { getElementById(id) { looked.push(id); return id === 'm124' ? { getElementsByClassName: () => [link] } : null; } },
    }, "Parser.parseBacklinks('124', '100');");
    assert.deepEqual(looked, ['m124', `pi${digits}`]);
    assert.equal(link.textContent, `>>${digits}${digits === '100' ? ' (OP)' : ''} →`);
  }
});

test('original remote JSON builder compares numeric post IDs with lexical decimal quote labels', () => {
  for (const pid of ['123', '000123', '000124']) {
    const globals = { Parser: { buildHTMLFromJSON: row => ({ lastElementChild: { no: row.no } }) },
      Main: { board: 'g' }, Config: { revealSpoilers: true, IDColor: false }, pid };
    const result = source(['Parser.buildPost'], globals, "Parser.buildPost([{no:123}], 'g', pid)");
    assert.equal(result?.no ?? null, pid === '000124' ? null : 123);
  }
});

test('original self detection is lexical; the rewrite additionally fences numeric aliases as cycle authority', () => {
  for (const pid of ['123', '000123']) {
    const result = source(['QuoteInline.isSelfQuote'], { QuoteInline: {}, Main: { board: 'g' }, pid,
      node: { parentNode: { nodeName: 'BLOCKQUOTE', id: 'm123', parentNode: { id: 'p123' } } },
    }, "QuoteInline.isSelfQuote(node, pid, 'g')");
    assert.equal(result, pid === '123');
    assert.equal(quotePostId(pid), '123');
  }
});

test('quote grammar retains decimal spellings while lookup authority remains exact and bounded', () => {
  for (const href of ['/rules#p', '/rules#pol', '/rules#pw/1', '/rules#ph']) assert.equal(postLinkUrl(href, page), true);
  for (const post of ['000123', `${'0'.repeat(30)}123`, '0009223372036854775807']) {
    for (const href of [`#p${post}`, `/g/thread/000100#p${post}`]) {
      const ref = quoteTarget(href, page);
      assert.equal(ref.post, post);
      assert.equal(quotePostId(ref.post), post.replace(/^0+/, ''));
    }
  }
  assert.deepEqual(quoteTarget('/g/thread/000123#p000123', page), { board: 'g', thread: '000123', post: '000123' });
  for (const value of ['0', '000', '+123', '-123', '123e0', '123\n', '１２３', '0009223372036854775808', '0'.repeat(513)]) {
    assert.equal(quotePostId(value), null, value);
  }
});

test('remote alias transport requests canonical fixed-origin endpoints and revalidates canonical wire identity', async () => {
  const alias = { ...page, post: '000123', thread: '000100' }, requests = [];
  const transport = new NativeQuotePreviewTransport({ origin: page.origin, now: () => 0,
    fetcher: async (url, options) => { requests.push({ url, options }); return { url, status: 200, redirected: false,
      headers: new Headers({ 'content-type': 'application/json' }), body: new Response(JSON.stringify(packet())).body }; },
    createWorker: () => ({ terminate() {}, postMessage(job) { queueMicrotask(() => this.onmessage({ data: parseQuotePreviewSnapshot(job.raw, job.context) })); } }),
  });
  const result = await transport.load(alias);
  assert.equal(result.status, 'ok');
  assert.equal(requests[0].url, 'https://board.example/_watch/g/post/123');
  assert.equal(requests[0].options.credentials, 'omit');
  assert.equal(requests[0].options.redirect, 'error');
  assert.equal(checkedQuotePreview(result, alias).status, 'ok');
  for (const patch of [{ post: '0000' }, { thread: '0000' }, { thread: 100 }, { thread: '000124' }]) {
    assert.equal((await transport.load({ ...alias, ...patch })).status, 'invalid-context');
  }
  assert.throws(() => previewUrl(alias), 'raw wire context never gains alias authority');
  const rawContext = { ...page, post: '123' };
  for (const changed of [packet('000123'), packet('123', '000100')]) {
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(changed), rawContext).status, 'invalid-preview');
  }
  const forged = structuredClone(result); forged.snapshot.post.no = '000123';
  assert.equal(checkedQuotePreview(forged, alias).status, 'invalid-preview');
});

test('atomic updater admits leading-zero comment anchors without accepting noncanonical packet IDs', () => {
  const value = { version: 2, board: 'g', thread: '100', closed: false, archived: false, sticky: false,
    replies: 1, images: 0, tail_size: 0, tail_id: null, posts: [
      { no: '100', file_deleted: false, html: html('100').replaceAll('reply', 'op') },
      { no: '123', file_deleted: false, html: html('123').replace('Remote safe text', '<a class="quotelink" href="#p000123">&gt;&gt;000123</a>') },
    ] };
  assert.equal(parseUpdaterSnapshot(JSON.stringify(value), page).status, 'ok');
  value.posts[1].no = '000123';
  assert.equal(parseUpdaterSnapshot(JSON.stringify(value), page).status, 'invalid-snapshot');
});
