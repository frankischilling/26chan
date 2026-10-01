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
} from '../../apps/public/static/global-search.v1.js';

class FakeNode {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = {};
    this.options = []; this.value = ''; this.disabled = false; this.textContent = ''; this.className = '';
  }
  addEventListener(type, listener) { this.listeners[type] = listener; }
  append(node) { this.children.push(node); }
  replaceChildren(...nodes) { this.children = nodes; this.html = []; }
  insertAdjacentHTML(_position, html) { (this.html ??= []).push(html); }
  insertAdjacentElement(_position, node) { this.after = node; }
  remove() { this.removed = true; }
}

function searchFixture({ hash = '', mobile = false, fetcher }) {
  const form = new FakeNode('form'), query = new FakeNode('input'), board = new FakeNode('select');
  const button = new FakeNode('button'), results = new FakeNode('div'), anchor = new FakeNode('div');
  board.options = [{ value: '' }, { value: 'a' }, { value: 'g' }];
  const fixed = new Map([['g-search-form', form], ['js-sf-qf', query], ['js-sf-bf', board], ['js-sf-btn', button], ['js-sf-results', results], ['delform', anchor]]);
  const root = {
    getElementById(id) { return id === 'js-sf-pl' ? (anchor.after?.removed ? null : anchor.after) : fixed.get(id) ?? null; },
    createElement(tag) { return new FakeNode(tag); },
  };
  const location = { href: `https://search.example/globalsearch.php${hash}`, hash };
  const historyCalls = [];
  const history = { replaceState(_state, _title, url) { historyCalls.push(url); location.href = url; location.hash = new URL(url).hash; } };
  const events = {};
  const scope = { addEventListener(type, listener) { events[type] = listener; } };
  const handle = mountGlobalSearch({ root, history, location, scope, fetcher, mobile: { matches: mobile } });
  return { handle, query, board, button, results, anchor, historyCalls, events };
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
  assert.equal(parseSearchHash(`#/${'x'.repeat(512)}`, boards).query.length, 512);
  assert.equal(searchHash('two words', '', 20), '#/two%20words/all/3');
  assert.equal(searchHash('owned', 'g', 0), '#/owned/g');
  assert.equal(pageToOffset(11), 0);
  assert.equal(offsetToPage(95), 1);
});

test('global search validates the reconstructed finite response contract', () => {
  const valid = { threads: [{ board: 'g', posts: [{ no: 1 }], html: '<section>owned</section>' }], offset: 0, nhits: 1 };
  assert.equal(parseSearchPayload(valid), valid);
  for (const invalid of [
    { ...valid, offset: 5 },
    { ...valid, nhits: 20001 },
    { ...valid, threads: Array(11).fill(valid.threads[0]) },
    { ...valid, threads: [{ ...valid.threads[0], board: '../j' }] },
    { ...valid, threads: [{ ...valid.threads[0], posts: [] }] },
    { ...valid, threads: [{ ...valid.threads[0], html: 'x'.repeat(200001) }] },
  ]) assert.throws(() => parseSearchPayload(invalid), SyntaxError);
});

test('search transport stays same-origin and supports cancellation', async () => {
  const calls = [];
  const fetcher = async (url, options) => {
    calls.push({ url, options });
    return new Response(JSON.stringify({ threads: [], offset: 10, nhits: 0 }), {
      headers: { 'content-type': 'application/json' },
    });
  };
  const data = await requestSearch({ query: 'owned space', board: 'g', offset: 10, fetcher });
  assert.deepEqual(data, { threads: [], offset: 10, nhits: 0 });
  assert.equal(calls[0].url, '/search/api?q=owned+space&b=g&o=10');
  assert.equal(calls[0].options.credentials, 'same-origin');
  assert.equal(calls[0].options.mode, 'same-origin');
  assert.equal(calls[0].options.redirect, 'error');

  const controller = new AbortController();
  const pending = requestSearch({
    query: 'owned', signal: controller.signal,
    fetcher: (_url, options) => new Promise((resolve, reject) => {
      options.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true });
    }),
  });
  controller.abort();
  await assert.rejects(pending, error => error.name === 'AbortError');
});

test('direct hashes restore scoped searches and source pagination on desktop and mobile', async () => {
  for (const mobile of [false, true]) {
    const calls = [];
    const fetcher = async (url, options) => {
      calls.push({ url, options });
      return new Response(JSON.stringify({
        threads: [{ board: 'g', posts: [{ no: 9 }], html: '<section>owned</section>' }], offset: 10, nhits: 24,
      }), { headers: { 'content-type': 'application/json' } });
    };
    const fixture = searchFixture({ hash: '#/owned/g/2', mobile, fetcher });
    assert.equal(fixture.query.value, 'owned'); assert.equal(fixture.board.value, 'g');
    assert.equal(calls[0].url, '/search/api?q=owned&b=g&o=10');
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(fixture.results.html, ['<section>owned</section>']);
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
