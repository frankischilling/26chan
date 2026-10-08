import test from 'node:test';
import assert from 'node:assert/strict';
import {
  SEARCH_LIMITS,
  offsetToPage,
  pageToOffset,
  parseSearchHash,
  parseSearchPayload,
  requestSearch,
  searchHash,
  mountGlobalSearch,
} from '../../apps/public/client/global-search.js';

const opHtml = (no, text = 'owned') => `<article class="postContainer opContainer" id="pc${no}"><div class="post op" id="p${no}"><div class="postInfo" id="pi${no}"></div><blockquote class="postMessage" id="m${no}">${text}</blockquote></div></article>`;

class FakeNode {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = {};
    this.options = []; this.value = ''; this.disabled = false; this.textContent = ''; this.className = ''; this.attributes = {};
  }
  addEventListener(type, listener) { this.listeners[type] = listener; }
  setAttribute(name, value) { this.attributes[name] = String(value); this[name] = String(value); if (name === 'class') this.className = String(value); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; this.html = []; }
  insertAdjacentElement(_position, node) { this.after = node; }
  remove() { this.removed = true; }
}

function searchFixture({ hash = '', mobile = false, fetcher }) {
  const form = new FakeNode('form'), query = new FakeNode('input'), board = new FakeNode('select');
  const button = new FakeNode('button'), results = new FakeNode('div'), anchor = new FakeNode('div'), context = new FakeNode('div');
  context.dataset.mediaOrigin = '';
  board.options = [{ value: '' }, { value: 'a' }, { value: 'g' }];
  const fixed = new Map([['g-search-form', form], ['js-sf-qf', query], ['js-sf-bf', board], ['js-sf-btn', button], ['js-sf-results', results], ['delform', anchor], ['search-context', context]]);
  const root = {
    getElementById(id) { return id === 'js-sf-pl' ? (anchor.after?.removed ? null : anchor.after) : fixed.get(id) ?? null; },
    createElement(tag) { return new FakeNode(tag); },
    createTextNode(value) { const node = new FakeNode('#text'); node.textContent = value; return node; },
  };
  const location = { href: `https://search.example/globalsearch.php${hash}`, hash };
  const historyCalls = [];
  const history = { replaceState(_state, _title, url) { historyCalls.push(url); location.href = url; location.hash = new URL(url).hash; } };
  const events = {};
  const scope = { addEventListener(type, listener) { events[type] = listener; } };
  const handle = mountGlobalSearch({ root, history, location, scope, fetcher, mobile: { matches: mobile } });
  return { handle, query, board, button, results, anchor, historyCalls, events, location };
}

test('global search keeps the original ten-by-ten hash grammar', () => {
  const boards = new Set(['a', 'g']);
  assert.equal(SEARCH_LIMITS.pageSize, 10);
  assert.equal(SEARCH_LIMITS.maxPages, 10);
  assert.deepEqual(parseSearchHash('#/owned/g/10', boards), { query: 'owned', board: 'g', offset: 90 });
  assert.deepEqual(parseSearchHash('#/owned/all/2', boards), { query: 'owned', board: '', offset: 10 });
  assert.deepEqual(parseSearchHash('#/owned/missing/3', boards), { query: 'owned', board: '', offset: 20 });
  assert.equal(parseSearchHash('#/%E0%A4%A', boards), null);
  assert.deepEqual(parseSearchHash(`#/${'x'.repeat(513)}`, boards), { query: '', board: '', offset: 0 });
  assert.equal(parseSearchHash(`#/${'x'.repeat(510)}`, boards).query.length, 510);
  assert.equal(searchHash('two words', '', 20), '#/two%20words/all/3');
  assert.equal(searchHash('owned', 'g', 0), '#/owned/g');
  assert.equal(pageToOffset(11), 0);
  assert.equal(offsetToPage(95), 1);
});

test('global search validates the reconstructed finite response contract', () => {
  const valid = { threads: [{ board: 'g', thread: '1', posts: [{ no: '1', html: opHtml('1') }] }], offset: 0, nhits: 1 };
  const parsed = parseSearchPayload(JSON.stringify(valid), { origin: 'https://search.example/' });
  assert.equal(parsed.threads[0].posts[0].no, '1');
  for (const invalid of [
    { ...valid, offset: 5 },
    { ...valid, nhits: 20001 },
    { ...valid, threads: Array(11).fill(valid.threads[0]) },
    { ...valid, threads: [{ ...valid.threads[0], board: '../j' }] },
    { ...valid, threads: [{ ...valid.threads[0], posts: [] }] },
    { ...valid, threads: [{ ...valid.threads[0], posts: [{ no: '1', html: '<script>alert(1)</script>' }] }] },
    { ...valid, extra: true },
  ]) assert.throws(() => parseSearchPayload(JSON.stringify(invalid), { origin: 'https://search.example/' }));
});

test('search transport stays same-origin and supports cancellation', async () => {
  const calls = [];
  const fetcher = async (url, options) => {
    calls.push({ url, options });
    return new Response(JSON.stringify({ threads: [], offset: 10, nhits: 0 }), {
      headers: { 'content-type': 'application/json' },
    });
  };
  const data = await requestSearch({ query: 'owned space', board: 'g', offset: 10, origin: 'https://search.example', fetcher });
  assert.deepEqual(data, { threads: [], offset: 10, nhits: 0 });
  assert.equal(calls[0].url, '/search/api?q=owned+space&b=g&o=10');
  assert.equal(calls[0].options.credentials, 'same-origin');
  assert.equal(calls[0].options.mode, 'same-origin');
  assert.equal(calls[0].options.redirect, 'error');

  const controller = new AbortController();
  const pending = requestSearch({
    query: 'owned', origin: 'https://search.example', signal: controller.signal,
    fetcher: (_url, options) => new Promise((resolve, reject) => {
      options.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true });
    }),
  });
  controller.abort();
  await assert.rejects(pending, error => error.name === 'AbortError');

  const oversized = new Response('{}', { headers: { 'content-type': 'application/json', 'content-length': String(SEARCH_LIMITS.responseBytes + 1) } });
  await assert.rejects(requestSearch({ query: 'owned', origin: 'https://search.example', fetcher: async () => oversized }), /response-limit/);
});

test('direct hashes restore scoped searches and source pagination on desktop and mobile', async () => {
  for (const mobile of [false, true]) {
    const calls = [];
    const fetcher = async (url, options) => {
      calls.push({ url, options });
      return new Response(JSON.stringify({
        threads: [{ board: 'g', thread: '9', posts: [{ no: '9', html: opHtml('9') }] }], offset: 10, nhits: 24,
      }), { headers: { 'content-type': 'application/json' } });
    };
    const fixture = searchFixture({ hash: '#/owned/g/2', mobile, fetcher });
    assert.equal(fixture.query.value, 'owned'); assert.equal(fixture.board.value, 'g');
    assert.equal(calls[0].url, '/search/api?q=owned&b=g&o=10');
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(fixture.results.children[0].id, 't9');
    assert.equal(fixture.results.children[0].dataset.board, 'g');
    assert.equal(fixture.results.children[0].children[0].children[0].href, '/g/');
    assert.equal(fixture.anchor.after.className, mobile ? 'mPagelist mobile' : 'pagelist desktop');
    assert.equal(fixture.anchor.after.children.find(node => node.className === 'pages').textContent, 'Page 2 / 3');
    assert.equal(fixture.historyCalls.at(-1), 'https://search.example/globalsearch.php#/owned/g/2');
  }
});

test('a new search aborts the previous workflow and global search omits the board parameter', async () => {
  const calls = [];
  const fetcher = (url, options) => {
    calls.push({ url, options });
    return new Promise((_resolve, reject) => options.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true }));
  };
  const fixture = searchFixture({ fetcher });
  fixture.handle.execute('first', 'g', 0);
  fixture.handle.execute('second', '', 0);
  assert.equal(calls.at(-2).options.signal.aborted, true);
  assert.equal(calls.at(-1).url, '/search/api?q=second');
  fixture.handle.destroy();
  assert.equal(calls.at(-1).options.signal.aborted, true);
});

test('a malformed replacement hash cancels the older request before showing its error', async () => {
  const calls = [];
  const fetcher = (url, options) => {
    calls.push({ url, options });
    return new Promise((_resolve, reject) => options.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true }));
  };
  const fixture = searchFixture({ hash: '#/first/g', fetcher });
  assert.equal(calls.length, 1);
  const old = calls[0].options.signal;
  const before = fixture.handle.state.sequence;
  fixture.location.hash = '#/%E0%A4%A';
  fixture.events.hashchange();
  assert.equal(old.aborted, true);
  assert.equal(fixture.handle.state.sequence, before + 1);
  assert.equal(fixture.results.children[0].textContent, 'Something went wrong.');
});

test('hash table matches unchanged pinned source methods, including UTF-16 and ToInt32', async () => {
  const { sourceSearch, hashCases, source } = await import('./helpers/global-search-source.mjs');
  const { createHash } = await import('node:crypto');
  assert.equal(source.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  assert.equal(source.file_sha256, '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31');
  assert.equal(source.sha256, '7c69839faf04ed5ccceed0cf4bb66644582a0417b4e1a21042898edd81b2c698');
  assert.equal(createHash('sha256').update(source.text).digest('hex'), source.sha256);
  assert.equal(Buffer.byteLength(source.text), source.byte_end - source.byte_start);
  for (const hash of hashCases) {
    assert.deepEqual(parseSearchHash(hash, new Set(['a', 'g'])), sourceSearch(hash), JSON.stringify(hash));
  }
  const { runInNewContext } = await import('node:vm');
  const methods = runInNewContext(`var Search = { pageSize: 10, maxPages: 10, ${source.text} exec() {} }; Search;`);
  for (const value of [-Infinity, -4294967294, -1, 0, 1, 1.5, 2, 10, 10.9, 11, 90, 95, Infinity, NaN]) {
    assert.equal(pageToOffset(value), methods.pageToOffset(value), `page ${value}`);
    assert.equal(offsetToPage(value), methods.offsetToPage(value), `offset ${value}`);
  }
});

test('source-derived table kills prefix-length, slashless and parseInt regressions', async () => {
  const { sourceSearch, hashCases } = await import('./helpers/global-search-source.mjs');
  const original = parseSearchHash.toString();
  const mutations = [
    ['prefix excluded from limit', 'hash.length >', 'hash.slice(2).length >'],
    ['slashless query accepted', "hash.split('/').slice(1)", "hash.slice(hash.startsWith('#/') ? 2 : 1).split('/')"],
    ['decimal parseInt instead of ToInt32', '0 | fragments[2]', 'Number.parseInt(fragments[2], 10)'],
  ];
  for (const [name, before, after] of mutations) {
    assert.ok(original.includes(before), name);
    const mutant = Function('SEARCH_LIMITS', 'pageToOffset', `return (${original.replace(before, after)});`)(SEARCH_LIMITS, pageToOffset);
    assert.ok(hashCases.some(hash => JSON.stringify(mutant(hash, new Set(['a', 'g']))) !== JSON.stringify(sourceSearch(hash))), name);
  }
});

test('clearing or over-limit replacement hashes cancel in-flight searches without stale output', async () => {
  for (const hash of ['#owned', `#/${'x'.repeat(511)}`]) {
    let resolve;
    let signal;
    const fixture = searchFixture({ hash: '#/first/g', fetcher: (_url, options) => {
      signal = options.signal;
      return new Promise(done => { resolve = done; });
    } });
    fixture.location.hash = hash;
    fixture.events.hashchange();
    assert.equal(signal.aborted, true);
    assert.equal(fixture.query.value, '');
    assert.equal(fixture.button.disabled, false);
    resolve(new Response(JSON.stringify({ threads: [], offset: 0, nhits: 0 }), { headers: { 'content-type': 'application/json' } }));
    await new Promise(done => setImmediate(done));
    assert.deepEqual(fixture.results.children, []);
  }
});
