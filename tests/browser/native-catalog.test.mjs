import test from 'node:test';
import assert from 'node:assert/strict';
import { CATALOG_LIMITS, parseNativeCatalog } from '../../apps/public/static/native-catalog.v1.js';

const catalog = threads => JSON.stringify([{ page: 1, threads }]);
const invalid = raw => assert.deepEqual(parseNativeCatalog(raw), { status: 'invalid-catalog' });

test('catalog numeric and quoted IDs retain adjacent values above 2^53 and the positive i64 maximum', () => {
  assert.deepEqual(parseNativeCatalog('[{"page":1,"threads":[{"no":9007199254740992},{"no":9007199254740993},{"no":9223372036854775807}]},{"page":0,"threads":[{"no":"1"}]}]'),
    { status: 'ok', posts: [{ no: '9007199254740992' }, { no: '9007199254740993' }, { no: '9223372036854775807' }, { no: '1' }] });
});

test('catalog projection preserves encoded literal fields and raw comment presence without parsing HTML', () => {
  const first = { no: '1', trip: '!literal', name: 'A &amp; B', id: 'id|[x]', sub: '"no":9007199254740993',
    filename: '<image>.png', com: '<span>&gt;&gt;2</span><br>text' };
  assert.deepEqual(parseNativeCatalog(catalog([{ ...first, time: 123, last_replies: [{ no: 22, com: 'ignored reply' }] },
    { no: '2' }, { no: '3', com: '' }, { no: '4', com: '<span></span>' }])),
  { status: 'ok', posts: [first, { no: '2' }, { no: '3', com: '' }, { no: '4', com: '<span></span>' }] });
});

test('escaped JSON keys preserve numeric IDs and cannot conceal duplicate members', () => {
  assert.deepEqual(parseNativeCatalog(String.raw`[{"page":1,"threads":[{"\u006e\u006f":9007199254740993}]}]`),
    { status: 'ok', posts: [{ no: '9007199254740993' }] });
  invalid(String.raw`[{"page":1,"threads":[{"no":1,"\u006e\u006f":2}]}]`);
  invalid('[{"page":1,"page":2,"threads":[]}]');
});

test('catalog identifiers reject zero, signs, exponents, decimals and values outside positive i64', () => {
  for (const id of ['0', '-0', '-1', '1.0', '1e0', '9223372036854775808', '"01"', '"+1"', '"1e0"', 'true', 'null', '[]', '{}']) {
    invalid(`[{"page":1,"threads":[{"no":${id}}]}]`);
  }
});

test('catalog parsing rejects malformed JSON and non-JSON whitespace without accepting a partial result', () => {
  for (const raw of ['', 'null', '{}', '[', '[{"page":1,"threads":[],}]', '[{"page":1,"threads":[]}]x',
    '[{"page":01,"threads":[]}]', '[{"page":1,"threads":[{"no":1,"name":"line\nbreak"}]}]',
    String.raw`[{"page":1,"threads":[{"no":1,"name":"\uZZZZ"}]}]`, '\u00a0[]', '[]\u2028']) invalid(raw);
  invalid(null);
  invalid(' '.repeat(CATALOG_LIMITS.source + 1));
});

test('catalog pages and thread identities remain bounded, unique and structurally valid', () => {
  for (const pages of [[{ page: 1, threads: [] }, { page: 1, threads: [] }],
    [{ page: -1, threads: [] }], [{ page: 1.5, threads: [] }], [{ page: '1', threads: [] }],
    [{ page: 2147483648, threads: [] }], [{ page: 1, threads: {} }],
    [{ page: 1, threads: [{ no: '1' }] }, { page: 2, threads: [{ no: '1' }] }]]) invalid(JSON.stringify(pages));
  assert.deepEqual(parseNativeCatalog('[]'), { status: 'ok', posts: [] });
  const pages = Array.from({ length: CATALOG_LIMITS.pages }, (_, page) => ({ page, threads: Array.from({ length: 4 },
    (_, i) => ({ no: String(page * 4 + i + 1) })) }));
  assert.equal(parseNativeCatalog(JSON.stringify(pages)).posts.length, CATALOG_LIMITS.posts);
  pages[0].threads.push({ no: '513' });
  invalid(JSON.stringify(pages));
  invalid(JSON.stringify(Array.from({ length: CATALOG_LIMITS.pages + 1 }, (_, page) => ({ page, threads: [] }))));
});

test('catalog field types and decoded string bounds fail closed instead of truncating match data', () => {
  for (const field of ['trip', 'name', 'id', 'sub', 'filename', 'com']) {
    for (const value of [null, 1, true, [], {}]) invalid(catalog([{ no: '1', [field]: value }]));
  }
  assert.equal(parseNativeCatalog(catalog([{ no: '1', name: 'x'.repeat(CATALOG_LIMITS.field),
    com: 'x'.repeat(CATALOG_LIMITS.comment) }])).status, 'ok');
  invalid(catalog([{ no: '1', name: 'x'.repeat(CATALOG_LIMITS.field + 1) }]));
  invalid(catalog([{ no: '1', com: 'x'.repeat(CATALOG_LIMITS.comment + 1) }]));
});

test('ignored catalog metadata cannot evade depth, container, member or total-value limits', () => {
  let deep = '0';
  for (let i = 0; i <= CATALOG_LIMITS.depth; i++) deep = `[${deep}]`;
  invalid(`[{"page":1,"threads":[],"extra":${deep}}]`);
  invalid(JSON.stringify([{ page: 1, threads: [], extra: Array(CATALOG_LIMITS.array + 1).fill(null) }]));
  invalid(JSON.stringify([{ page: 1, threads: [], extra: Object.fromEntries(Array.from({ length: CATALOG_LIMITS.keys + 1 }, (_, i) => [`k${i}`, null])) }]));
  invalid(JSON.stringify([{ page: 1, threads: [], extra: { ['x'.repeat(CATALOG_LIMITS.key + 1)]: 0 } }]));
  invalid(JSON.stringify([{ page: 1, threads: [], extra: Array.from({ length: CATALOG_LIMITS.array }, () => Array(32).fill(null)) }]));
});

test('catalog prototype-shaped keys cannot become projected fields or mutate object prototypes', () => {
  const raw = '[{"page":1,"threads":[{"no":1,"__proto__":{"catalogPolluted":true},"constructor":{"prototype":{"catalogPolluted":true}}}]}]';
  assert.deepEqual(parseNativeCatalog(raw), { status: 'ok', posts: [{ no: '1' }] });
  assert.equal(Object.prototype.catalogPolluted, undefined);
});
