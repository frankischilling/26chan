import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { Worker } from 'node:worker_threads';
import { CATALOG_FILTER_LIMITS, CatalogFilterMatcher, readCatalogFilters, writeCatalogFilters,
  runCatalogFilterJob } from '../../apps/public/static/catalog-filter-core.v1.js';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-filters-reference.json', import.meta.url)));
const cards = Object.entries(reference.catalog.threads).map(([id, value]) => ({ id,
  text: `<b>${value.sub}</b>: ${value.teaser}`, author: value.author,
  ...(value.file ? { file: value.file } : {}), ...(value.trip ? { trip: value.trip } : {}),
  ...(value.capcode ? { capcode: value.capcode } : {}),
}));
for (const row of reference.matches.filter(row => !Object.keys(row.stored).length)) {
  test(`worker matches independent public catalog vector ${row.label}`, () => {
    const result = JSON.parse(runCatalogFilterJob(JSON.stringify({ version: 1, board: 'demo', cards, rules: Object.values(row.filters) })));
    assert.equal(result.status, 'ok');
    const visible = new Map(row.value.cards.map(card => [card.id.slice(7), card]));
    const expected = cards.filter(card => !visible.has(card.id) || visible.get(card.id).highlighted).map(card => card.id);
    assert.deepEqual(result.matches.map(match => match.id), expected);
    assert.ok(result.matches.every(match => match.filter === 0));
  });
}
const rule = { active: 1, pattern: 'sheet', boards: '', hidden: 1, top: 0 };
test('catalog storage and request bounds reject malformed, reserved and excessive data', () => {
  assert.deepEqual(readCatalogFilters(null), { status: 'ok', rules: [] });
  for (const raw of ['[]', '{', '{"__proto__":{}}', JSON.stringify({ 0: { ...rule, active: '1' } }),
    JSON.stringify({ 0: { ...rule, pattern: 'x'.repeat(CATALOG_FILTER_LIMITS.pattern + 1) } }),
    JSON.stringify({ 0: { ...rule, boards: 'x'.repeat(CATALOG_FILTER_LIMITS.boards + 1) } }),
    'x'.repeat(CATALOG_FILTER_LIMITS.storage + 1)]) assert.equal(readCatalogFilters(raw).status, 'invalid-settings');
  assert.equal(writeCatalogFilters(Array(CATALOG_FILTER_LIMITS.rules + 1).fill(rule)).status, 'invalid-settings');
  for (const change of [{ board: '../demo' }, { cards: [cards[0], cards[0]] }, { cards: [{ ...cards[0], id: '9223372036854775808' }] },
    { cards: [{ ...cards[0], text: 'x'.repeat(CATALOG_FILTER_LIMITS.field + 1) }] }, { cards: Array(513).fill(cards[0]) }]) {
    assert.equal(JSON.parse(runCatalogFilterJob(JSON.stringify({ version: 1, board: 'demo', cards, rules: [rule], ...change }))).status, 'invalid-request');
  }
  assert.equal(JSON.parse(runCatalogFilterJob(JSON.stringify({ version: 1, board: 'demo', cards, rules: [{ ...rule, pattern: '/[/' }] }))).status, 'invalid-filter');
  assert.equal(writeCatalogFilters([]).raw, null);
});

function respondingWorker(reply, observed) {
  return () => ({
    postMessage() { queueMicrotask(() => this.onmessage?.({ data: reply })); },
    terminate() { observed.terminated++; },
  });
}
test('untrusted worker results cannot invent IDs, reorder matches or select an inactive or unscoped rule', async () => {
  const rows = [rule, { ...rule, active: 0 }, { ...rule, boards: 'other' }];
  for (const reply of [{ status: 'ok', matches: [{ id: '123', filter: 0 }] },
    { status: 'ok', matches: [{ id: cards[0].id, filter: 0 }, { id: cards[0].id, filter: 0 }] },
    { status: 'ok', matches: [{ id: cards[1].id, filter: 0 }, { id: cards[0].id, filter: 0 }] },
    { status: 'ok', matches: [{ id: cards[0].id, filter: 1 }] },
    { status: 'ok', matches: [{ id: cards[0].id, filter: 2 }] },
    { status: 'ok', matches: [{ id: cards[0].id, filter: -1 }] }]) {
    const observed = { terminated: 0 };
    const matcher = new CatalogFilterMatcher({ createWorker: respondingWorker(JSON.stringify(reply), observed) });
    assert.equal((await matcher.match(rows, 'demo', cards)).status, 'invalid-result');
    assert.equal(observed.terminated, 1);
  }
});
test('cancellation, errors and unavailable workers never evaluate on the caller thread', async () => {
  let created = 0, terminated = 0;
  const controller = new AbortController(); controller.abort();
  const matcher = new CatalogFilterMatcher({ createWorker() { created++; return { postMessage() {}, terminate() { terminated++; } }; } });
  assert.equal((await matcher.match([rule], 'demo', cards, { signal: controller.signal })).status, 'cancelled');
  assert.equal(created, 0);
  const running = new AbortController();
  const pending = matcher.match([rule], 'demo', cards, { signal: running.signal }); running.abort();
  assert.equal((await pending).status, 'cancelled'); assert.equal(terminated, 1);
  const unavailable = new CatalogFilterMatcher({ createWorker() { throw new Error('denied'); } });
  assert.equal((await unavailable.match([rule], 'demo', cards)).status, 'unavailable');
});

function realWorker() {
  const path = new URL('../../apps/public/static/catalog-filter-core.v1.js', import.meta.url).href;
  const worker = new Worker(`const { parentPort } = require('node:worker_threads');
    import(${JSON.stringify(path)}).then(module => parentPort.on('message', raw => parentPort.postMessage(module.runCatalogFilterJob(raw))));`, { eval: true });
  const adapter = { postMessage: raw => worker.postMessage(raw), terminate: () => void worker.terminate() };
  worker.on('message', data => adapter.onmessage?.({ data }));
  worker.on('error', error => adapter.onerror?.({ error, preventDefault() {} }));
  return adapter;
}
test('an actual pathological regex worker is terminated at the wall-clock deadline and a fresh healthy job works', async () => {
  const matcher = new CatalogFilterMatcher({ createWorker: realWorker });
  const start = performance.now();
  const result = await matcher.match([{ ...rule, pattern: '/^(a+)+$/' }], 'demo', [{ id: '1', text: 'a'.repeat(50000) + '!' }]);
  assert.equal(result.status, 'timeout');
  assert.ok(performance.now() - start < 3000);
  assert.deepEqual(await matcher.match([rule], 'demo', cards), { status: 'ok', matches: [{ id: cards[0].id, filter: 0 }] });
});
